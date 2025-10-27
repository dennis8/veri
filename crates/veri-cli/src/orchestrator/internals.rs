use super::coverage::CoverageService;
use super::telemetry::TelemetryService;
use super::watch::WatchAdapter;
use crate::cli::{Cli, ExitCode};
use anyhow::Result;
use veri_core::cache::{compute_config_digest, CacheKey};
use veri_core::compatibility::CompatibilityMatrix;
use veri_core::config::Config;
use veri_core::diagnostics::{DiagnosticReporter, VeriDiagnostic};
use veri_core::import_graph::ImportGraphBuilder;
use veri_core::planner::{PlannerConfig, TestPlanner};
use veri_core::python_launcher::PythonRuntime;
use veri_core::python_worker::{PythonWorker, TestRunOptions};
use veri_core::scheduler::{SchedulerConfig, SchedulingStrategy, TestScheduler};
use veri_core::security::{SecurityConfig, SecurityScanner};
use veri_core::telemetry::{ErrorCategory, RunEvent};
use veri_core::worker_pool::{WorkerPool, WorkerPoolConfig};

fn detect_project_root_from_paths(paths: &[String]) -> Option<std::path::PathBuf> {
    use std::path::{Path, PathBuf};
    if paths.is_empty() {
        return None;
    }
    let mut cand = PathBuf::from(&paths[0]);
    if cand.is_file() {
        cand = cand
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
    }
    if cand.file_name().map(|n| n == "tests").unwrap_or(false) {
        if let Some(parent) = cand.parent() {
            cand = parent.to_path_buf();
        }
    }
    // Normalize empty path to current directory to avoid returning ""
    if cand.as_os_str().is_empty() {
        cand = PathBuf::from(".");
    }
    for _ in 0..3 {
        if cand.join("pyproject.toml").exists()
            || cand.join("pytest.ini").exists()
            || cand.join("setup.cfg").exists()
        {
            return Some(cand.clone());
        }
        if !cand.pop() {
            break;
        }
    }
    None
}

fn normalize_paths(paths: &[String], work_dir: &std::path::Path) -> Vec<String> {
    use std::path::Path;
    let mut out = Vec::new();
    for p in paths {
        let pb = Path::new(p);
        if let Ok(rel) = pb.strip_prefix(work_dir) {
            out.push(rel.to_string_lossy().to_string());
        } else if pb.is_absolute() {
            // Keep absolute paths as-is
            out.push(p.clone());
        } else {
            // Try to normalize common case: <work_dir>/tests → tests
            if p.starts_with(&format!("{}/", work_dir.display())) {
                out.push(p[work_dir.display().to_string().len() + 1..].to_string());
            } else if let Some(name) = work_dir.file_name() {
                let name = name.to_string_lossy();
                let prefix = format!("{}/", name);
                if p.starts_with(&prefix) {
                    out.push(p[prefix.len()..].to_string());
                } else {
                    out.push(p.clone());
                }
            } else {
                out.push(p.clone());
            }
        }
    }
    out
}

pub(super) fn run_pytest_engine(cli: &Cli, config: &Config) -> Result<ExitCode> {
    println!("🔄 Using pytest engine for compatibility");

    // Create Python worker
    let work_dir = detect_project_root_from_paths(&cli.paths).unwrap_or(std::env::current_dir()?);
    let work_dir = std::fs::canonicalize(&work_dir).unwrap_or(work_dir);
    let cache_dir = work_dir.join(".veri").join("cache");
    let python_runtime_cfg = config.python();
    let python_runtime = PythonRuntime::from_config(&work_dir, &python_runtime_cfg);
    let worker = PythonWorker::from_runtime(&work_dir, &cache_dir, &python_runtime);

    // Build pytest arguments from CLI
    let mut pytest_args = Vec::new();

    // Add basic flags
    if cli.verbose > 0 {
        for _ in 0..cli.verbose {
            pytest_args.push("-v".to_string());
        }
    }
    if cli.quiet {
        pytest_args.push("-q".to_string());
    }
    if cli.no_capture {
        pytest_args.push("-s".to_string());
    }
    if cli.exitfirst {
        pytest_args.push("-x".to_string());
    }
    if let Some(maxfail) = cli.maxfail {
        pytest_args.push("--maxfail".to_string());
        pytest_args.push(maxfail.to_string());
    }

    // Add filters
    if let Some(keyword) = &cli.keyword {
        pytest_args.push("-k".to_string());
        pytest_args.push(keyword.clone());
    }
    if let Some(marker) = &cli.marker {
        pytest_args.push("-m".to_string());
        pytest_args.push(marker.clone());
    }

    // Add output options
    if let Some(junit_xml) = &cli.junit_xml {
        pytest_args.push("--junit-xml".to_string());
        pytest_args.push(junit_xml.to_string_lossy().to_string());
    }

    // Add parallel workers
    if let Some(workers) = &cli.workers {
        if workers != "1" {
            pytest_args.push("-n".to_string());
            pytest_args.push(workers.clone());
        }
    }

    // Add ignores
    for ig in &cli.ignore {
        pytest_args.push("--ignore".to_string());
        pytest_args.push(ig.clone());
    }

    // Add paths (normalized to selected work_dir)
    let norm_paths = normalize_paths(&cli.paths, &work_dir);
    pytest_args.extend(norm_paths);

    // If no paths and not --all, run current directory
    if pytest_args.is_empty() && !cli.all {
        pytest_args.push(".".to_string());
    }

    // Execute via Python worker
    let exit_code = worker.run_pytest_engine(&pytest_args)?;

    match exit_code {
        0 => Ok(ExitCode::Success),
        1 => Ok(ExitCode::TestFailure),
        2 => Ok(ExitCode::Interrupted),
        4 => Ok(ExitCode::UsageError),
        _ => Ok(ExitCode::InternalError),
    }
}

/// Check compatibility and handle fallback to pytest if needed
/// Returns Some(ExitCode) if we should exit early (either success or fallback to pytest)
fn check_compatibility_and_fallback(
    cli: &Cli,
    config: &Config,
    worker: &PythonWorker,
    compatibility_matrix: &CompatibilityMatrix,
) -> Result<Option<ExitCode>> {
    let plugins = worker.get_pytest_plugins().unwrap_or_default();
    let compatibility_report = compatibility_matrix.generate_report(worker, &plugins)?;

    // If explicitly requested, always print the compatibility report and exit
    if cli.compatibility_report {
        compatibility_report.print_report(!config.no_color());
        return Ok(Some(ExitCode::Success));
    }

    // Otherwise, print when verbose or issues detected
    if cli.verbose > 0
        || !compatibility_report.environment.overall_supported
        || compatibility_report.plugin_check.needs_fallback
    {
        compatibility_report.print_report(!config.no_color());
        println!();
    }

    // Auto-fallback to pytest if incompatible plugins detected
    if compatibility_report.plugin_check.needs_fallback && !cli.disable_allowlist {
        println!(
            "🔄 Automatically falling back to pytest engine due to plugin compatibility issues"
        );
        return Ok(Some(run_pytest_engine(cli, config)?));
    }

    Ok(None)
}

/// Validate plugins and run security checks
/// Returns Some(ExitCode) if validation fails and we should exit
fn validate_plugins_and_security(
    cli: &Cli,
    config: &Config,
    worker: &PythonWorker,
    security_config: &SecurityConfig,
    diagnostics: &mut DiagnosticReporter,
    telemetry: &mut TelemetryService,
) -> Result<Option<ExitCode>> {
    // Security: Validate pytest plugins
    if security_config.enforce_allowlist {
        println!("🔒 Validating pytest plugins...");
        match worker.get_pytest_plugins() {
            Ok(plugins) => {
                let validation_result = security_config.validate_plugins(&plugins);

                if validation_result.has_blocked_plugins() {
                    if !cli.disable_allowlist {
                        // Add diagnostic and exit when not overridden
                        diagnostics.add(VeriDiagnostic::PluginIncompatible {
                            plugin_name: validation_result.blocked.join(", "),
                            version: "unknown".to_string(),
                            reason: "Plugin not in allowlist".to_string(),
                            fallback_suggested: true,
                        });

                        if let Some(warning) = validation_result.get_warning_message() {
                            eprintln!("{}", warning);
                        }

                        println!("🚨 Blocked plugins detected. Use --disable-allowlist to override (not recommended)");
                        telemetry.record_error(ErrorCategory::PluginError);
                        return Ok(Some(ExitCode::UsageError));
                    } else {
                        // Overridden: log a warning but do not add fatal diagnostic
                        if let Some(warning) = validation_result.get_warning_message() {
                            eprintln!("{}", warning);
                        }
                    }
                } else {
                    println!(
                        "✅ All {} plugins are allowed",
                        validation_result.allowed.len()
                    );
                }

                // Run security scanner for additional warnings
                let security_warnings = SecurityScanner::scan_plugins(&plugins);
                for warning in &security_warnings {
                    println!("{}", warning.format(!config.no_color()));
                }
            }
            Err(e) => {
                println!("⚠️  Could not validate plugins: {}", e);
                telemetry.record_error(ErrorCategory::PluginError);
            }
        }
    } else {
        println!("ℹ️  Plugin allowlist enforcement disabled");
    }

    Ok(None)
}

pub(super) fn run_veri_engine(
    cli: &Cli,
    config: &Config,
    security_config: &SecurityConfig,
    compatibility_matrix: &CompatibilityMatrix,
    telemetry: &mut TelemetryService,
    watch_adapter: &dyn WatchAdapter,
) -> Result<ExitCode> {
    println!("🚀 Using veri engine for maximum speed");

    let work_dir = detect_project_root_from_paths(&cli.paths).unwrap_or(std::env::current_dir()?);
    let work_dir = std::fs::canonicalize(&work_dir).unwrap_or(work_dir);
    let cache_dir = work_dir.join(".veri").join("cache");
    let python_runtime_cfg = config.python();
    let python_runtime = PythonRuntime::from_config(&work_dir, &python_runtime_cfg);

    // Handle watch mode
    if cli.watch {
        return watch_adapter.run(cli, &work_dir, &cache_dir, &python_runtime);
    }

    let worker = PythonWorker::from_runtime(&work_dir, &cache_dir, &python_runtime);

    // Initialize diagnostics reporter
    let mut diagnostics = DiagnosticReporter::new(cli.quiet);

    // Check compatibility and handle fallback
    if let Some(exit_code) =
        check_compatibility_and_fallback(cli, config, &worker, compatibility_matrix)?
    {
        return Ok(exit_code);
    }

    // Check Python environment
    worker.check_environment(&mut diagnostics)?;

    // Validate plugins and run security checks
    if let Some(exit_code) = validate_plugins_and_security(
        cli,
        config,
        &worker,
        security_config,
        &mut diagnostics,
        telemetry,
    )? {
        return Ok(exit_code);
    }

    // Check if we need to collect tests (first run or --all)
    let needs_collection = cli.all || !worker.has_valid_cache();

    // Determine paths to collect (normalized to work_dir)
    let collection_paths = if !cli.paths.is_empty() {
        normalize_paths(&cli.paths, &work_dir)
    } else {
        vec![] // Empty means collect all
    };

    // Collect tests (only once!)
    let (tests_index, collection_time_ms) = if needs_collection {
        let collection_start = std::time::Instant::now();
        println!("📋 Collecting tests...");

        // Collect tests
        let tests_index = worker.collect_tests(&collection_paths, &cli.ignore)?;

        // Check for collection errors
        worker.check_collection_errors(&tests_index, &mut diagnostics);

        println!("✅ Collected {} tests", tests_index.tests.len());

        if !tests_index.collection_errors.is_empty() {
            println!(
                "⚠️  {} collection errors encountered",
                tests_index.collection_errors.len()
            );
            for error in &tests_index.collection_errors {
                eprintln!("  {}: {}", error.path, error.message);
            }
        }

        // Build import graph only when impact analysis is needed
        if !cli.all {
            println!("🔍 Building import graph...");
            let mut graph_builder =
                ImportGraphBuilder::with_runtime(&work_dir, &cache_dir, &python_runtime);
            let (imports_graph, _revdeps_graph, _module_map) = graph_builder.build_graphs()?;

            println!(
                "✅ Built import graph with {} edges",
                imports_graph.edges.len()
            );
            if !imports_graph.dynamic_imports.is_empty() {
                println!(
                    "⚠️  {} dynamic imports detected",
                    imports_graph.dynamic_imports.len()
                );
            }
        }

        // If this was just collection (--all with no other action), we're done
        if cli.all && cli.paths.is_empty() && !should_run_tests(cli) {
            return Ok(ExitCode::Success);
        }

        let collection_time_ms = collection_start.elapsed().as_millis() as u64;
        (tests_index, collection_time_ms)
    } else {
        // Load cached tests (no collection time measured)
        let tests_index = worker.collect_tests(&collection_paths, &cli.ignore)?;
        (tests_index, 0)
    };

    // Report any diagnostics from collection phase
    diagnostics.report_all()?;

    // If there were critical errors, exit early
    if diagnostics.has_errors() {
        return Ok(ExitCode::InternalError);
    }

    // Determine which tests to run
    let mut nodeids_to_run = if cli.all {
        tests_index.tests.iter().map(|t| t.nodeid.clone()).collect()
    } else {
        // Load or build graphs for impact analysis
        let mut graph_builder =
            ImportGraphBuilder::with_runtime(&work_dir, &cache_dir, &python_runtime);
        let (imports_graph, revdeps_graph, module_map) = match graph_builder.load_cached_graphs()? {
            Some(graphs) => graphs,
            None => {
                println!("🔍 Building import graph...");
                let graphs = graph_builder.build_graphs()?;
                println!("✅ Built import graph with {} edges", graphs.0.edges.len());
                graphs
            }
        };

        select_tests_to_run(
            &tests_index,
            &imports_graph,
            &revdeps_graph,
            &module_map,
            cli,
        )?
    };

    // Fallback: if nothing selected but tests exist, run all tests
    if nodeids_to_run.is_empty() && !tests_index.tests.is_empty() {
        println!("ℹ️  No impacted tests detected; defaulting to run all tests");
        nodeids_to_run = tests_index.tests.iter().map(|t| t.nodeid.clone()).collect();
    }

    if nodeids_to_run.is_empty() {
        let mut diagnostics = DiagnosticReporter::new(cli.quiet);
        diagnostics.add(VeriDiagnostic::no_tests_found(
            cli.keyword.as_deref(),
            cli.marker.as_deref(),
            &cli.paths,
        ));
        diagnostics.report_all()?;
        return Ok(ExitCode::UsageError);
    }

    println!("🎯 Running {} selected tests", nodeids_to_run.len());

    // Always use the worker pool (worker_count may be 1)
    let worker_count = parse_worker_count(&cli.workers)?;

    // Configure test run options
    let run_options = TestRunOptions {
        verbose: cli.verbose > 0,
        quiet: cli.quiet,
        no_capture: cli.no_capture,
        exitfirst: cli.exitfirst,
        maxfail: cli.maxfail,
        junit_xml: cli.junit_xml.clone(),
        workers: Some(worker_count.to_string()),
        ignore: cli.ignore.clone(),
        coverage: cli.cov,
        coverage_xml: cli.cov || cli.cov_merge_full,
        coverage_html: false,                          // Can be configured later
        coverage_source_dirs: vec!["src".to_string()], // Default, can be made configurable
        coverage_omit: vec![
            "*/tests/*".to_string(),
            "*/test_*".to_string(),
            "*/__pycache__/*".to_string(),
            "*/venv/*".to_string(),
            "*/.venv/*".to_string(),
        ],
    };

    let coverage_service = CoverageService::new(cli, &work_dir, &cache_dir);
    coverage_service.initialize(&nodeids_to_run)?;

    // Execute tests using the scheduler and worker pool
    let start_time = std::time::Instant::now();
    let exec_config = ParallelExecutionConfig {
        work_dir: &work_dir,
        cache_dir: &cache_dir,
        verbose: cli.verbose > 0,
        config,
        python_runtime: python_runtime.clone(),
    };
    let test_result = execute_tests_parallel(
        &nodeids_to_run,
        &tests_index,
        worker_count,
        &run_options,
        &exec_config,
    );

    let execution_duration = start_time.elapsed();

    // Get actual Python version from cache key
    let config_digest = compute_config_digest(config)?;
    let cache_key = CacheKey::from_environment(config_digest, Some(&python_runtime.launcher))?;
    let python_version = Some(cache_key.python_version);

    // Record telemetry for this run
    let run_event = RunEvent {
        test_count: nodeids_to_run.len() as u32,
        worker_count: worker_count as u32,
        collection_time_ms,
        execution_time_ms: execution_duration.as_millis() as u64,
        coverage_enabled: cli.cov || cli.cov_merge_full,
        watch_mode: cli.watch,
        ci_mode: cli.ci,
        python_version,
        features_used: {
            let mut features = Vec::new();
            if cli.cov || cli.cov_merge_full {
                features.push("coverage".to_string());
            }
            if cli.watch {
                features.push("watch".to_string());
            }
            if worker_count > 1 {
                features.push("parallel".to_string());
            }
            if !needs_collection {
                features.push("impact_analysis".to_string());
            }
            features
        },
    };

    telemetry.record_run(run_event);

    // Record telemetry based on test result
    if test_result.is_err() {
        telemetry.record_error(ErrorCategory::ExecutionError);
    }

    coverage_service.finalize(cli, &nodeids_to_run)?;

    test_result
}

fn should_run_tests(cli: &Cli) -> bool {
    // Check if any flags indicate we should actually run tests, not just collect
    cli.keyword.is_some()
        || cli.marker.is_some()
        || cli.last_failed
        || !cli.paths.is_empty()
        || cli.watch
}

fn select_tests_to_run(
    tests_index: &veri_core::python_worker::TestsIndex,
    imports_graph: &veri_core::import_graph::ImportsGraph,
    revdeps_graph: &veri_core::import_graph::ReverseDepsGraph,
    module_map: &veri_core::import_graph::ModuleMap,
    cli: &Cli,
) -> Result<Vec<String>> {
    // If --all is specified, run all tests
    if cli.all {
        return Ok(tests_index.tests.iter().map(|t| t.nodeid.clone()).collect());
    }

    // Create planner
    let work_dir = std::env::current_dir()?;
    let cache_dir = work_dir.join(".veri").join("cache");
    let planner = TestPlanner::new(&work_dir, &cache_dir);

    // Determine changed files (for now, use a simple git diff approach)
    let changed_files = get_changed_files()?;

    // If we have manual filters (keyword, marker, paths), apply them first
    let mut selected = Vec::new();
    for test in &tests_index.tests {
        let mut include = true;

        // Apply keyword filter
        if let Some(keyword) = &cli.keyword {
            include &= test.nodeid.contains(keyword) || test.function.contains(keyword);
        }

        // Apply marker filter
        if let Some(marker) = &cli.marker {
            include &= test.markers.contains(marker);
        }

        // Apply path filter
        if !cli.paths.is_empty() {
            include &= cli
                .paths
                .iter()
                .any(|path| test.path.starts_with(path) || test.nodeid.contains(path));
        }

        if include {
            selected.push(test.nodeid.clone());
        }
    }

    // If we have manual filters, use those selections and skip impact analysis
    if cli.keyword.is_some() || cli.marker.is_some() || !cli.paths.is_empty() {
        return Ok(selected);
    }

    // Use impact-aware planning if no manual filters
    if changed_files.is_empty() {
        // No changes detected, run nothing unless --last-failed or other flags
        if cli.last_failed {
            // TODO: Implement last-failed logic when we have failure tracking
            println!("ℹ️  No last-failed tracking yet - running all tests");
            return Ok(tests_index.tests.iter().map(|t| t.nodeid.clone()).collect());
        } else {
            println!("ℹ️  No changed files detected - use --all to run all tests");
            return Ok(Vec::new());
        }
    }

    // Use the planner for impact analysis
    let selection = planner.plan_test_selection(
        &changed_files,
        tests_index,
        revdeps_graph,
        module_map,
        imports_graph,
    )?;

    // Print selection summary and diagnostics
    let mut diagnostics = DiagnosticReporter::new(cli.quiet);

    if selection.should_broaden {
        let _original_percentage =
            (selection.selected_nodeids.len() as f64 / selection.total_tests as f64) * 100.0;
        let planner_config = PlannerConfig::default();

        diagnostics.add(VeriDiagnostic::selection_broadened(
            selection.selected_nodeids.len(),
            selection.total_tests,
            planner_config.broaden_threshold,
            selection
                .broaden_reason
                .clone()
                .unwrap_or_else(|| "Unknown".to_string()),
        ));
    }

    // Check for dynamic imports and add diagnostics
    for dynamic_import in &imports_graph.dynamic_imports {
        diagnostics.add(VeriDiagnostic::dynamic_import_detected(
            &dynamic_import.from_module,
            &dynamic_import.reason,
            selection.should_broaden,
        ));
    }

    // Report diagnostics (warnings only unless there are errors)
    diagnostics.report_all()?;

    // Print explanation if verbose
    if cli.verbose > 0 || cli.explain {
        println!("{}", planner.format_explain(&selection));
    }

    Ok(selection.selected_nodeids)
}

/// Get changed files using git (simple implementation for Phase 4)
fn get_changed_files() -> Result<Vec<String>> {
    use std::process::Command;

    let work_dir = std::env::current_dir()?;

    // Get git root
    let git_root_output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&work_dir)
        .output();

    let git_root = match git_root_output {
        Ok(output) if output.status.success() => {
            std::path::PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
        }
        _ => return Ok(Vec::new()), // No git repo
    };

    // Get changed files from git root
    let output = Command::new("git")
        .args(["diff", "--name-only", "HEAD"])
        .current_dir(&git_root)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let files: Vec<String> = stdout
                .lines()
                .filter(|line| !line.is_empty())
                .filter(|line| line.ends_with(".py") || line.ends_with("conftest.py"))
                .filter_map(|line| {
                    let full_path = git_root.join(line);

                    // Check if this file is under our current working directory
                    if let Ok(relative_path) = full_path.strip_prefix(&work_dir) {
                        Some(relative_path.to_string_lossy().replace('\\', "/"))
                    } else if full_path == work_dir.join(std::path::Path::new(line).file_name()?) {
                        // File is directly in working directory
                        std::path::Path::new(line)
                            .file_name()
                            .map(|name| name.to_string_lossy().to_string())
                    } else {
                        None
                    }
                })
                .collect();
            Ok(files)
        }
        _ => {
            // Git not available or no repository - return empty
            Ok(Vec::new())
        }
    }
}

/// Parse worker count from string
fn parse_worker_count(workers: &Option<String>) -> Result<usize> {
    match workers {
        Some(w) => {
            if w == "auto" {
                Ok(num_cpus::get().max(1))
            } else {
                w.parse::<usize>()
                    .map_err(|_| anyhow::anyhow!("Invalid worker count: {}", w))
                    .and_then(|n| {
                        if n == 0 {
                            Err(anyhow::anyhow!("Worker count must be > 0"))
                        } else {
                            Ok(n)
                        }
                    })
            }
        }
        None => Ok(num_cpus::get().max(1)),
    }
}

/// Helper used for tests and documenting routing: we always go through the
/// worker pool, even for a single worker and a single selected test.
#[cfg_attr(not(test), allow(dead_code))]
fn should_use_pool(_worker_count: usize, _selected_len: usize) -> bool {
    true
}

/// Configuration for parallel test execution
pub struct ParallelExecutionConfig<'a> {
    pub work_dir: &'a std::path::Path,
    pub cache_dir: &'a std::path::Path,
    pub verbose: bool,
    pub config: &'a Config,
    pub python_runtime: PythonRuntime,
}

/// Setup test scheduler with historical timing data
fn setup_scheduler(
    worker_count: usize,
    nodeids: &[String],
    tests_index: &veri_core::python_worker::TestsIndex,
    exec_config: &ParallelExecutionConfig,
) -> Result<(TestScheduler, Vec<veri_core::scheduler::TestBatch>)> {
    // Configure scheduler
    let scheduler_config = SchedulerConfig {
        strategy: SchedulingStrategy::Balanced,
        worker_count,
        enable_long_pole_detection: true,
        long_pole_threshold_ms: 5000,
        enable_fail_first: true,
        max_batch_duration_ms: 30000,
    };

    // Create scheduler and load timing data
    let mut scheduler = TestScheduler::new(scheduler_config);

    // Try to load historical timings
    if let Ok(timings_data) =
        veri_core::schemas::TimingsData::load_from_cache(exec_config.cache_dir)
    {
        if let Err(e) = scheduler.load_timings(&timings_data) {
            if exec_config.verbose {
                println!("⚠️  Could not load timing data: {}", e);
            }
        } else if exec_config.verbose {
            println!("📊 Loaded historical timing data");
        }
    }

    // Schedule tests into batches
    let batches = scheduler.schedule_tests(nodeids, tests_index)?;

    Ok((scheduler, batches))
}

/// Persist test timing data for future scheduling optimization
fn persist_timing_data(
    results: &[veri_core::worker_pool::BatchResult],
    worker_count: usize,
    cache_dir: &std::path::Path,
) -> Result<()> {
    if results.is_empty() {
        return Ok(());
    }

    use chrono::Utc;
    use std::collections::HashMap;

    // Build this run timings
    let mut test_timings: HashMap<String, veri_core::schemas::TestTiming> = HashMap::new();
    for r in results {
        for t in &r.per_test {
            let outcome = match t.outcome.as_str() {
                "passed" => veri_core::schemas::TestOutcome::Passed,
                "failed" => veri_core::schemas::TestOutcome::Failed,
                "skipped" => veri_core::schemas::TestOutcome::Skipped,
                _ => veri_core::schemas::TestOutcome::Error,
            };
            test_timings.insert(
                t.nodeid.clone(),
                veri_core::schemas::TestTiming {
                    nodeid: t.nodeid.clone(),
                    setup_duration: 0.0,
                    call_duration: (t.duration_ms as f64) / 1000.0,
                    teardown_duration: 0.0,
                    total_duration: (t.duration_ms as f64) / 1000.0,
                    outcome,
                    worker_id: None,
                },
            );
        }
    }

    let run = veri_core::schemas::TimingRun {
        run_id: format!("parallel-{}", Utc::now().timestamp()),
        started_at: Utc::now(),
        finished_at: Utc::now(),
        workers: worker_count as u32,
        test_timings,
    };

    // Load existing timings if present
    let mut timings = match veri_core::schemas::TimingsData::load_from_cache(cache_dir) {
        Ok(t) => t,
        Err(_) => veri_core::schemas::TestTimings::new(),
    };

    // Append run
    timings.runs.push(run);

    // Update aggregated_timings incrementally
    for r in results {
        for t in &r.per_test {
            let entry = timings
                .aggregated_timings
                .entry(t.nodeid.clone())
                .or_insert(veri_core::schemas::AggregatedTiming {
                    nodeid: t.nodeid.clone(),
                    run_count: 0,
                    avg_duration: 0.0,
                    min_duration: f64::MAX,
                    max_duration: 0.0,
                    p50_duration: 0.0,
                    p95_duration: 0.0,
                    last_duration: 0.0,
                    stability: 1.0,
                });
            let d = (t.duration_ms as f64) / 1000.0;
            entry.run_count += 1;
            // Incremental average
            entry.avg_duration += (d - entry.avg_duration) / (entry.run_count as f64);
            if d < entry.min_duration {
                entry.min_duration = d;
            }
            if d > entry.max_duration {
                entry.max_duration = d;
            }
            entry.last_duration = d;
            // Simple approximations for percentiles
            entry.p50_duration = entry.avg_duration;
            entry.p95_duration = entry.max_duration;
            // Stability heuristic: decay towards 0 on failures, towards 1 on passes
            if t.outcome == "failed" || t.outcome == "error" {
                entry.stability = (entry.stability * 0.8).max(0.0);
            } else {
                entry.stability = (entry.stability * 0.8 + 0.2).min(1.0);
            }
        }
    }

    if let Ok(json) = timings.to_json() {
        let path = cache_dir.join("timings.json");
        let _ = std::fs::write(&path, json);
    }

    Ok(())
}

/// Print batch failure details with stdout/stderr tails
fn print_batch_failure(result: &veri_core::worker_pool::BatchResult) {
    println!(
        "\n--- Batch failure: worker {} ({} tests, {:.1}s, exit {}) ---",
        result.worker_id,
        result.nodeids.len(),
        result.duration.as_secs_f64(),
        result.exit_code
    );

    if !result.stdout.is_empty() {
        println!("[stdout tail]");
        for line in result
            .stdout
            .lines()
            .rev()
            .take(80)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            println!("{}", line);
        }
    }

    if !result.stderr.is_empty() {
        eprintln!("[stderr tail]");
        for line in result
            .stderr
            .lines()
            .rev()
            .take(80)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            eprintln!("{}", line);
        }
    }
}

/// Process results and print summary
fn process_and_summarize_results(
    results: &[veri_core::worker_pool::BatchResult],
    total_duration: std::time::Duration,
    worker_count: usize,
    verbose: bool,
) -> (i32, usize, usize) {
    let mut total_exit_code = 0;
    let mut failed_batches = 0;
    let mut total_tests_run = 0;

    // Process individual batch results
    for result in results {
        total_tests_run += result.nodeids.len();

        if result.exit_code != 0 {
            failed_batches += 1;
            if total_exit_code == 0 {
                total_exit_code = result.exit_code;
            }
            print_batch_failure(result);
        }

        if verbose {
            println!(
                "Worker {} completed {} tests in {:.1}s (exit: {})",
                result.worker_id,
                result.nodeids.len(),
                result.duration.as_secs_f64(),
                result.exit_code
            );
        }
    }

    // Print summary
    println!(
        "✅ Completed {} tests in {:.1}s using {} workers",
        total_tests_run,
        total_duration.as_secs_f64(),
        worker_count
    );

    if failed_batches > 0 {
        println!(
            "❌ {} of {} worker batches reported failures",
            failed_batches,
            results.len()
        );
    }

    // Outcome rollup
    let mut passed = 0u32;
    let mut skipped = 0u32;
    let mut failed = 0u32;
    let mut error = 0u32;

    for r in results {
        for t in &r.per_test {
            match t.outcome.as_str() {
                "passed" => passed += 1,
                "skipped" => skipped += 1,
                "failed" => failed += 1,
                "error" => error += 1,
                _ => {}
            }
        }
    }

    if passed + skipped + failed + error > 0 {
        println!(
            "Summary: {} passed, {} skipped, {} failed, {} error ({} total)",
            passed,
            skipped,
            failed,
            error,
            passed + skipped + failed + error
        );
    }

    (total_exit_code, failed_batches, total_tests_run)
}

/// Execute tests in parallel using scheduler and worker pool
pub fn execute_tests_parallel(
    nodeids: &[String],
    tests_index: &veri_core::python_worker::TestsIndex,
    worker_count: usize,
    run_options: &TestRunOptions,
    exec_config: &ParallelExecutionConfig,
) -> Result<ExitCode> {
    use std::time::Instant;

    println!("⚡ Scheduling tests across {} workers", worker_count);

    // Setup scheduler and create test batches
    let (scheduler, batches) = setup_scheduler(worker_count, nodeids, tests_index, exec_config)?;

    if batches.is_empty() {
        return Ok(ExitCode::Success);
    }

    // Show scheduling information
    let stats = scheduler.get_scheduling_stats(&batches);
    if exec_config.verbose {
        println!("{}", scheduler.format_explain(&batches, &stats));
    } else {
        println!(
            "📋 Scheduled {} tests across {} workers",
            stats.total_tests, stats.total_workers
        );
        println!(
            "⏱️  Estimated duration: {:.1}s (load balance: {:.1}%)",
            stats.total_estimated_duration_ms as f64 / 1000.0,
            stats.load_balance_ratio * 100.0
        );
    }

    // Configure worker pool
    let worker_cfg = exec_config.config.worker.clone().unwrap_or_default();
    let mut pool_config = WorkerPoolConfig::default();
    pool_config.worker_count = worker_count;
    pool_config.startup_timeout =
        std::time::Duration::from_secs(worker_cfg.startup_timeout_sec.unwrap_or(30));
    pool_config.execution_timeout =
        std::time::Duration::from_secs(worker_cfg.execution_timeout_sec.unwrap_or(300));
    pool_config.heartbeat_interval =
        std::time::Duration::from_secs(worker_cfg.heartbeat_interval_sec.unwrap_or(10));
    pool_config.work_dir = exec_config.work_dir.to_path_buf();
    pool_config.cache_dir = exec_config.cache_dir.to_path_buf();
    pool_config.apply_runtime(&exec_config.python_runtime);

    // Create and start worker pool
    let mut worker_pool = WorkerPool::new(pool_config);
    worker_pool.start()?;

    // Submit batches to worker pool
    let start_time = Instant::now();
    for (i, batch) in batches.iter().enumerate() {
        let batch_id = format!("batch_{}", i);
        worker_pool.submit_batch(batch_id, batch.clone(), run_options.clone())?;
    }

    println!("🚀 Executing tests...");

    // Wait for completion
    let results = worker_pool.wait_for_completion(Some(std::time::Duration::from_secs(600)))?;
    let total_duration = start_time.elapsed();

    // Persist timing data for future scheduling optimization
    persist_timing_data(&results, worker_count, exec_config.cache_dir)?;

    // Process results and print summary
    let (total_exit_code, _failed_batches, _total_tests_run) =
        process_and_summarize_results(&results, total_duration, worker_count, exec_config.verbose);

    // Shutdown worker pool
    worker_pool.shutdown()?;

    // Return appropriate exit code
    match total_exit_code {
        0 => Ok(ExitCode::Success),
        1 => Ok(ExitCode::TestFailure),
        2 => Ok(ExitCode::Interrupted),
        4 => Ok(ExitCode::UsageError),
        _ => Ok(ExitCode::InternalError),
    }
}

pub(super) fn print_explanation(cli: &Cli, config: &Config) -> Result<()> {
    println!("=== veri Execution Plan ===");
    println!();

    // Import graph status
    let work_dir = std::env::current_dir()?;
    let cache_dir = work_dir.join(".veri").join("cache");
    let python_runtime_cfg = config.python();
    let python_runtime = PythonRuntime::from_config(&work_dir, &python_runtime_cfg);

    // Cache key components - now with real implementation using the launcher
    let config_digest = compute_config_digest(config)?;
    let cache_key = CacheKey::from_environment(config_digest, Some(&python_runtime.launcher))?;
    cache_key.print_explanation();
    println!();

    // Configuration summary
    println!("Configuration:");
    println!("  Engine: {}", cli.engine);
    println!("  Workers: {}", config.workers.as_deref().unwrap_or("auto"));
    println!("  Cache dir: {}", config.cache_dir().display());
    println!("  Log level: {}", config.log_level());
    println!();

    let graph_builder = ImportGraphBuilder::with_runtime(&work_dir, &cache_dir, &python_runtime);

    println!("Import Graph Status:");
    match graph_builder.load_cached_graphs()? {
        Some((imports_graph, _revdeps_graph, module_map)) => {
            println!("  Status: Cached graphs available");
            println!("  Modules: {}", module_map.modules.len());
            println!("  Import edges: {}", imports_graph.edges.len());
            println!("  Dynamic imports: {}", imports_graph.dynamic_imports.len());
            println!(
                "  Unresolved imports: {}",
                imports_graph.unresolved_imports.len()
            );

            if !imports_graph.dynamic_imports.is_empty() {
                println!("  ⚠️  Dynamic imports detected - may trigger broadening for safety");
            }
        }
        None => {
            println!("  Status: No cached graphs - will build on first run");
        }
    }
    println!();

    // Selection logic with impact analysis
    if cli.all {
        println!("Selection: Running ALL tests (--all specified)");
    } else if cli.last_failed {
        println!("Selection: Running last failed tests");
    } else if cli.keyword.is_some() || cli.marker.is_some() || !cli.paths.is_empty() {
        if let Some(keyword) = &cli.keyword {
            println!("Selection: Keyword filter: '{}'", keyword);
        }
        if let Some(marker) = &cli.marker {
            println!("Selection: Marker filter: '{}'", marker);
        }
        if !cli.paths.is_empty() {
            println!("Selection: Path filter: {:?}", cli.paths);
        }
    } else {
        println!("Selection: Impact-aware (based on changed files)");

        // Show changed files
        match get_changed_files() {
            Ok(changed_files) => {
                if changed_files.is_empty() {
                    println!("  Changed files: None detected");
                    println!("  Impacted tests: None (no tests will run)");
                } else {
                    println!("  Changed files:");
                    for file in &changed_files {
                        println!("    - {}", file);
                    }

                    // Try to show impact analysis if graphs are available
                    if let Ok(Some((imports_graph, revdeps_graph, module_map))) =
                        graph_builder.load_cached_graphs()
                    {
                        let worker =
                            PythonWorker::from_runtime(&work_dir, &cache_dir, &python_runtime);
                        if worker.has_valid_cache() {
                            if let Ok(tests_index) = worker.collect_tests(&[], &[]) {
                                let planner = TestPlanner::new(&work_dir, &cache_dir);
                                if let Ok(selection) = planner.plan_test_selection(
                                    &changed_files,
                                    &tests_index,
                                    &revdeps_graph,
                                    &module_map,
                                    &imports_graph,
                                ) {
                                    println!("  Impact Analysis:");
                                    println!(
                                        "    Selected tests: {} of {}",
                                        selection.selected_nodeids.len(),
                                        selection.total_tests
                                    );
                                    if selection.should_broaden {
                                        println!(
                                            "    ⚠️  Broadened: {}",
                                            selection
                                                .broaden_reason
                                                .as_deref()
                                                .unwrap_or("Unknown")
                                        );
                                    }
                                }
                            }
                        }
                    } else {
                        println!("  Impacted tests: (will compute from import graph on first run)");
                    }
                }
            }
            Err(_) => {
                println!("  Changed files: (git not available - will check filesystem)");
            }
        }
    }

    // Invalidation rules
    println!();
    println!("Invalidation Rules:");
    println!("  1. Test file changed → run its tests");
    println!("  2. Source file changed → run tests that import it (via reverse deps)");
    println!("  3. conftest.py changed → run tests in that directory scope");
    println!("  4. Dynamic import detected → broaden selection for safety");
    println!("  5. Selection > 60% of total → run all tests");

    // Performance insights
    println!();
    println!("Performance Insights:");
    if cli.all {
        println!("  • Full test run - no impact analysis overhead");
    } else {
        println!("  • Impact analysis will save time on subsequent runs");
        println!("  • First run builds cache (may be slower)");
        println!("  • Watch mode provides sub-second feedback");
    }

    // Common troubleshooting
    println!();
    println!("Common Issues & Solutions:");
    println!("  • No tests found: Check test file naming (test_*.py or *_test.py)");
    println!("  • Tests not selected: Use -v to see selection reasoning");
    println!("  • Import errors: Run with --engine pytest to bypass analysis");
    println!("  • Slow performance: Check for syntax errors preventing caching");

    // Help links
    println!();
    println!("Documentation:");
    println!("  • Full guide: https://docs.veri.dev/");
    println!("  • Troubleshooting: https://docs.veri.dev/troubleshooting");
    println!("  • Configuration: https://docs.veri.dev/config");

    Ok(())
}

#[allow(dead_code)]
fn print_planned_execution(cli: &Cli, config: &Config) -> Result<()> {
    println!(
        "veri v{} - ultra-fast pytest-compatible test runner",
        env!("CARGO_PKG_VERSION")
    );
    println!();

    if cli.watch {
        println!("⚡ Watch mode enabled - will monitor file changes");
    }

    if cli.engine.to_string() == "pytest" {
        println!("🔄 Using pytest engine for compatibility");
    } else {
        println!("🚀 Using veri engine for maximum speed");
    }

    // Show what would be done
    println!();
    println!("Planned actions:");
    println!(
        "  1. Load configuration from: {}",
        cli.config
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "veri.toml or pyproject.toml".to_string())
    );

    if cli.all {
        println!("  2. Collect ALL tests (--all specified)");
    } else {
        println!("  2. Analyze changed files and compute impacted tests");
    }

    if let Some(workers) = &config.workers {
        println!("  3. Execute tests using {} workers", workers);
    } else {
        println!("  3. Execute tests using auto-detected worker count");
    }

    if config.cov.unwrap_or(false) {
        println!("  4. Collect coverage data");
        if config.cov_merge_full.unwrap_or(false) {
            println!("     - Merge with existing coverage for full report");
        }
    }

    if let Some(junit_path) = &config.junit_xml {
        println!("  5. Write JUnit XML to: {}", junit_path.display());
    }

    if let Some(jsonl_path) = &config.jsonl {
        println!("  6. Write JSONL events to: {}", jsonl_path.display());
    }

    println!();

    if !cli.paths.is_empty() {
        println!("Test paths/patterns:");
        for path in &cli.paths {
            println!("  - {}", path);
        }
        println!();
    }

    // Show exit codes
    println!("Exit codes:");
    println!("  0: All tests passed");
    println!("  1: Some tests failed");
    println!("  2: Test execution was interrupted");
    println!("  3: Internal error occurred");
    println!("  4: Usage error (bad arguments, config, etc.)");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_worker_count_auto_nonzero() {
        let wc = parse_worker_count(&Some("auto".to_string())).expect("parse auto");
        assert!(wc > 0, "auto should resolve to > 0 workers");
    }

    #[test]
    fn test_should_use_pool_always_true() {
        assert!(should_use_pool(1, 1));
        assert!(should_use_pool(1, 0));
        assert!(should_use_pool(4, 100));
    }
}
