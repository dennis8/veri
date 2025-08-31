mod cli;
#[cfg(test)]
mod cli_tests;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands, Engine, ExitCode};
use log::{info, warn};
use std::process;
use veri_core::cache::{compute_config_digest, CacheKey};
use veri_core::compatibility::CompatibilityMatrix;
use veri_core::config::Config;
use veri_core::coverage::{CoverageCollector, CoverageConfig, CoverageFormat};
use veri_core::diagnostics::{DiagnosticReporter, VeriDiagnostic};
use veri_core::event_stream::{generate_run_id, CIReporter};
use veri_core::flaky::{FlakyConfig, FlakyManager};
use veri_core::import_graph::ImportGraphBuilder;
use veri_core::planner::{PlannerConfig, TestPlanner};
use veri_core::python_worker::{PythonWorker, TestRunOptions};
use veri_core::scheduler::{SchedulerConfig, SchedulingStrategy, TestScheduler};
use veri_core::security::{SecurityConfig, SecurityScanner};
use veri_core::sharder::{SharderConfig, TestSharder};
use veri_core::telemetry::{ErrorCategory, RunEvent, TelemetryClient};
use veri_core::watch::{WatchConfig, WatchSession};
use veri_core::worker_pool::{WorkerPool, WorkerPoolConfig};

fn main() {
    let exit_code = match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::InternalError
        }
    };

    process::exit(exit_code.into());
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();

    // Initialize logging early
    init_logging(&cli)?;

    // Load configuration
    let mut config = Config::load(cli.config.as_deref())?;
    config.apply_cli_args(
        cli.all,
        cli.watch,
        cli.keyword.clone(),
        cli.marker.clone(),
        cli.workers.clone(),
        cli.last_failed,
        cli.junit_xml.clone(),
        cli.jsonl.clone(),
        cli.maxfail,
        cli.verbose,
        cli.quiet,
        cli.cov,
        cli.cov_merge_full,
        cli.no_capture,
        cli.engine.to_string(),
        cli.no_network,
        cli.disable_allowlist,
    );

    info!("veri v{} starting", env!("CARGO_PKG_VERSION"));

    // Initialize security configuration
    let security_config = SecurityConfig::from_config(&config);

    // Load compatibility matrix
    let work_dir = std::env::current_dir()?;
    let cache_dir = work_dir.join(".veri").join("cache");
    let compatibility_matrix =
        CompatibilityMatrix::load_or_default(work_dir.join("veri-compatibility.toml"))
            .unwrap_or_default();

    // Initialize flaky test manager
    let flaky_config = FlakyConfig {
        auto_retry: config.auto_retry.unwrap_or(true),
        retry_count: config.retry_count.unwrap_or(1),
        flaky_db_path: cache_dir.join("flaky_tests.json"),
        ..Default::default()
    };
    let mut flaky_manager = FlakyManager::new(flaky_config);
    if let Err(e) = flaky_manager.load() {
        warn!("Could not load flaky test database: {}", e);
    }

    // Handle --flaky-report early and exit
    if cli.flaky_report {
        println!("📊 Flaky Test Report");
        println!("====================");
        let report = flaky_manager.generate_report();
        println!("Total tests tracked: {}", report.total_tests);
        println!(
            "Flaky tests: {} ({:.1}%)",
            report.total_flaky, report.flaky_percentage
        );
        if report.total_flaky > 0 {
            println!("\nTop flaky tests:");
            for h in report.flaky_tests.iter().take(10) {
                println!("  - {} (score {:.2})", h.nodeid, h.flaky_score);
            }
            if !report.recommendations.is_empty() {
                println!("\nRecommendations:");
                for rec in &report.recommendations {
                    println!("  {}", rec);
                }
            }
        } else {
            println!("✅ No flaky tests recorded yet");
        }
        return Ok(ExitCode::Success);
    }

    // Initialize telemetry client (disabled by default)
    let telemetry_config = veri_core::telemetry::TelemetryConfig {
        enabled: config.is_telemetry_enabled(),
        endpoint: config.telemetry().endpoint,
        collection_interval: config.telemetry().collection_interval.unwrap_or(300),
        collect_performance: true,
        collect_usage: true,
        max_queue_size: 1000,
    };
    let telemetry_client = TelemetryClient::new(telemetry_config);

    // Handle telemetry status flag early
    if cli.telemetry_status {
        telemetry_client.print_status(!config.no_color());
        return Ok(ExitCode::Success);
    }

    // Handle version flag (clap handles this automatically with --version)
    // Handle help flag (clap handles this automatically with --help)

    // Handle subcommands first
    if let Some(command) = &cli.command {
        return handle_subcommand(command, &config, &cli);
    }

    // Print configuration in explain mode
    if cli.explain {
        print_explanation(&cli, &config)?;
        if cli.engine == Engine::Pytest {
            println!("Note: Using --engine pytest - no impact analysis will be performed");
        }
        return Ok(ExitCode::Success);
    }

    // Handle engine selection
    match cli.engine {
        Engine::Pytest => {
            // Complete handoff to pytest
            run_pytest_engine(&cli, &config)
        }
        Engine::Veri => {
            // Use veri's fast engine with pytest compatibility layer
            run_veri_engine(
                &cli,
                &config,
                &security_config,
                &compatibility_matrix,
                &mut flaky_manager,
                &mut telemetry_client.clone(),
            )
        }
    }
}

fn detect_project_root_from_paths(paths: &[String]) -> Option<std::path::PathBuf> {
    use std::path::{Path, PathBuf};
    if paths.is_empty() { return None; }
    let mut cand = PathBuf::from(&paths[0]);
    if cand.is_file() {
        cand = cand.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
    }
    if cand.file_name().map(|n| n == "tests").unwrap_or(false) {
        if let Some(parent) = cand.parent() { cand = parent.to_path_buf(); }
    }
    for _ in 0..3 {
        if cand.join("pyproject.toml").exists()
            || cand.join("pytest.ini").exists()
            || cand.join("setup.cfg").exists()
        {
            return Some(cand.clone());
        }
        if !cand.pop() { break; }
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

fn run_pytest_engine(cli: &Cli, _config: &Config) -> Result<ExitCode> {
    println!("🔄 Using pytest engine for compatibility");

    // Create Python worker
    let work_dir = detect_project_root_from_paths(&cli.paths).unwrap_or(std::env::current_dir()?);
    let work_dir = std::fs::canonicalize(&work_dir).unwrap_or(work_dir);
    let worker = PythonWorker::new(&work_dir, work_dir.join(".veri").join("cache"));

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

fn run_veri_engine(
    cli: &Cli,
    config: &Config,
    security_config: &SecurityConfig,
    compatibility_matrix: &CompatibilityMatrix,
    _flaky_manager: &mut FlakyManager,
    telemetry_client: &mut TelemetryClient,
) -> Result<ExitCode> {
    println!("🚀 Using veri engine for maximum speed");

    let work_dir = detect_project_root_from_paths(&cli.paths).unwrap_or(std::env::current_dir()?);
    let work_dir = std::fs::canonicalize(&work_dir).unwrap_or(work_dir);
    let cache_dir = work_dir.join(".veri").join("cache");

    // Handle watch mode
    if cli.watch {
        return run_watch_mode(
            cli,
            config,
            &work_dir,
            &cache_dir,
            &mut telemetry_client.clone(),
        );
    }

    let worker = PythonWorker::new(&work_dir, &cache_dir);

    // Initialize diagnostics reporter
    let mut diagnostics = DiagnosticReporter::new(cli.quiet);

    // Check Python environment compatibility
    let plugins = worker.get_pytest_plugins().unwrap_or_default();
    let compatibility_report = compatibility_matrix.generate_report(&worker, &plugins)?;

    // If explicitly requested, always print the compatibility report and exit
    if cli.compatibility_report {
        compatibility_report.print_report(!config.no_color());
        return Ok(ExitCode::Success);
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
        return run_pytest_engine(cli, config);
    }

    // Check Python environment
    worker.check_environment(&mut diagnostics)?;

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
                        telemetry_client.record_error(ErrorCategory::PluginError);
                        return Ok(ExitCode::UsageError);
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
                telemetry_client.record_error(ErrorCategory::PluginError);
            }
        }
    } else {
        println!("ℹ️  Plugin allowlist enforcement disabled");
    }

    // Check if we need to collect tests (first run or --all)
    let needs_collection = cli.all || !worker.has_valid_cache();

    if needs_collection {
        println!("📋 Collecting tests...");

        // Determine paths to collect (normalized to work_dir)
        let collection_paths = if !cli.paths.is_empty() {
            normalize_paths(&cli.paths, &work_dir)
        } else {
            vec![] // Empty means collect all
        };

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
            let mut graph_builder = ImportGraphBuilder::new(&work_dir, &cache_dir);
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
    }

    // Load collected tests
    let collection_paths = if !cli.paths.is_empty() {
        normalize_paths(&cli.paths, &work_dir)
    } else {
        vec![]
    };
    let tests_index = worker.collect_tests(&collection_paths, &cli.ignore)?;

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
        let mut graph_builder = ImportGraphBuilder::new(&work_dir, &cache_dir);
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
            config,
        )?
    };

    // Fallback: if nothing selected but tests exist, run all tests
    if nodeids_to_run.is_empty() && !tests_index.tests.is_empty() {
        println!("ℹ️  No impacted tests detected; defaulting to run all tests");
        nodeids_to_run = tests_index
            .tests
            .iter()
            .map(|t| t.nodeid.clone())
            .collect();
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

    // Parse worker count
    let requested_workers = parse_worker_count(&cli.workers)?;
    let worker_count = if std::env::var_os("VERI_EXPERIMENTAL_WORKERPOOL").is_some() {
        requested_workers
    } else {
        1usize
    };

    // Configure test run options
    let run_options = TestRunOptions {
        verbose: cli.verbose > 0,
        quiet: cli.quiet,
        no_capture: cli.no_capture,
        exitfirst: cli.exitfirst,
        maxfail: cli.maxfail,
        junit_xml: cli.junit_xml.clone(),
        workers: Some("1".to_string()), // Each worker handles their batch sequentially
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

    // Initialize coverage if enabled
    let coverage_config = if cli.cov || cli.cov_merge_full {
        Some(CoverageConfig {
            enabled: true,
            merge_full: cli.cov_merge_full,
            output_formats: vec![CoverageFormat::Xml, CoverageFormat::Json],
            output_dir: work_dir.join("reports"),
            source_dirs: vec![work_dir.join("src")],
            omit_patterns: vec![
                "*/tests/*".to_string(),
                "*/test_*".to_string(),
                "*/__pycache__/*".to_string(),
                "*/venv/*".to_string(),
                "*/.venv/*".to_string(),
            ],
        })
    } else {
        None
    };

    let coverage_collector = coverage_config
        .as_ref()
        .map(|config| CoverageCollector::new(config.clone(), cache_dir.clone(), work_dir.clone()));

    // Initialize coverage for selected tests
    if let Some(collector) = &coverage_collector {
        collector.initialize_coverage(&nodeids_to_run)?;
    }

    // Execute tests using scheduler and worker pool if we have multiple workers
    let start_time = std::time::Instant::now();
    let test_result = if worker_count == 1 || nodeids_to_run.len() == 1 {
        // Single worker execution - use direct approach
        let exec = worker.run_tests(&nodeids_to_run, &run_options)?;
        if exec.exit_code != 0 {
            println!("\n--- Worker stdout (tail) ---");
            for line in exec.stdout.lines().rev().take(50).collect::<Vec<_>>().into_iter().rev() {
                println!("{}", line);
            }
            eprintln!("\n--- Worker stderr (tail) ---");
            for line in exec.stderr.lines().rev().take(50).collect::<Vec<_>>().into_iter().rev() {
                eprintln!("{}", line);
            }

            // Fallback: if execution failed with usage error, retry via pytest engine
            if exec.exit_code == 4 {
                println!("\nℹ️  Retrying via pytest engine due to execution error (code 4)...");
                return run_pytest_engine(cli, config);
            }
        }

        // Outcome rollup for single-worker path
        if !exec.per_test.is_empty() {
            let mut passed=0u32; let mut skipped=0u32; let mut failed=0u32; let mut error=0u32;
            for t in &exec.per_test { match t.outcome.as_str() { "passed"=>passed+=1, "skipped"=>skipped+=1, "failed"=>failed+=1, "error"=>error+=1, _=>{} } }
            let total = exec.per_test.len();
            println!(
                "Summary: {} passed, {} skipped, {} failed, {} error ({} total)",
                passed, skipped, failed, error, total
            );
        }
        match exec.exit_code {
            0 => Ok(ExitCode::Success),
            1 => Ok(ExitCode::TestFailure),
            2 => Ok(ExitCode::Interrupted),
            4 => Ok(ExitCode::UsageError),
            _ => Ok(ExitCode::InternalError),
        }
    } else {
        // Multi-worker execution - use scheduler and worker pool
        execute_tests_parallel(
            &nodeids_to_run,
            &tests_index,
            worker_count,
            &run_options,
            &work_dir,
            &cache_dir,
            cli.verbose > 0,
            config,
        )
    };

    let execution_duration = start_time.elapsed();

    // Record telemetry for this run
    let run_event = RunEvent {
        test_count: nodeids_to_run.len() as u32,
        worker_count: worker_count as u32,
        collection_time_ms: 0, // TODO: Measure collection time separately
        execution_time_ms: execution_duration.as_millis() as u64,
        coverage_enabled: cli.cov || cli.cov_merge_full,
        watch_mode: cli.watch,
        ci_mode: cli.ci,
        python_version: worker.get_pytest_plugins().ok().map(|_| "3.12".to_string()), // TODO: Get actual Python version
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

    telemetry_client.record_run(run_event);

    // Record telemetry based on test result
    if test_result.is_err() {
        telemetry_client.record_error(ErrorCategory::ExecutionError);
    }

    // Process coverage if enabled
    if let Some(collector) = coverage_collector {
        if cli.cov || cli.cov_merge_full {
            // Collect coverage data from the test run
            let coverage_map = collector.collect_coverage(&nodeids_to_run)?;

            // Save coverage map to cache
            collector.save_coverage_map(&coverage_map)?;

            // Generate full report if requested
            if cli.cov_merge_full {
                collector.generate_full_report(&coverage_map)?;
                println!("📊 Generated full coverage report in reports/");
            } else if cli.cov {
                println!("📊 Incremental coverage data collected and cached");
            }
        }
    }

    test_result
}

fn run_watch_mode(
    cli: &Cli,
    _config: &Config,
    work_dir: &std::path::Path,
    cache_dir: &std::path::Path,
    _telemetry_client: &mut TelemetryClient,
) -> Result<ExitCode> {
    println!("👀 Starting watch mode...");

    // Configure watch settings
    let watch_config = WatchConfig {
        debounce_delay: std::time::Duration::from_millis(150),
        max_wait_time: std::time::Duration::from_millis(500),
        respect_gitignore: true,
        enable_tui: !cli.quiet,
        verbose: cli.verbose > 0,
        ..Default::default()
    };

    // Configure test run options
    let run_options = TestRunOptions {
        verbose: cli.verbose > 0,
        quiet: cli.quiet,
        no_capture: cli.no_capture,
        exitfirst: cli.exitfirst,
        maxfail: cli.maxfail,
        junit_xml: cli.junit_xml.clone(),
        workers: Some("1".to_string()), // Use single worker in watch mode for speed
        ignore: cli.ignore.clone(),
        coverage: cli.cov,
        coverage_xml: cli.cov || cli.cov_merge_full,
        coverage_html: false,
        coverage_source_dirs: vec!["src".to_string()],
        coverage_omit: vec![
            "*/tests/*".to_string(),
            "*/test_*".to_string(),
            "*/__pycache__/*".to_string(),
            "*/venv/*".to_string(),
            "*/.venv/*".to_string(),
        ],
    };

    // Ensure we have collected tests first
    let worker = PythonWorker::new(work_dir, cache_dir);
    if !worker.has_valid_cache() {
        println!("📋 Initial collection required...");
        match worker.collect_tests(&[], &[]) {
            Ok(_tests_index) => {
                println!("✅ Initial collection completed");
            }
            Err(e) => {
                println!("⚠️  Initial collection failed: {}", e);
                println!("   Watch mode will continue but may have limited functionality");
            }
        }

        // Build import graphs
        println!("🔍 Building import graph...");
        let mut graph_builder = ImportGraphBuilder::new(work_dir, cache_dir);
        let mut diagnostics = DiagnosticReporter::new(false);

        match graph_builder.build_graphs() {
            Ok((imports_graph, _revdeps_graph, _module_map)) => {
                println!(
                    "✅ Built import graph with {} edges",
                    imports_graph.edges.len()
                );

                // Check for potential issues that might affect analysis
                if !imports_graph.unresolved_imports.is_empty() {
                    let missing_files: Vec<String> = imports_graph
                        .unresolved_imports
                        .iter()
                        .map(|u| format!("{} (from {})", u.import_name, u.from_module))
                        .collect();

                    diagnostics.add(VeriDiagnostic::ImportGraphBuildFailed {
                        error_count: imports_graph.unresolved_imports.len(),
                        syntax_errors: Vec::new(),
                        missing_files,
                    });
                }
            }
            Err(e) => {
                // Create a diagnostic for graph build failure
                diagnostics.add(VeriDiagnostic::ImportGraphBuildFailed {
                    error_count: 1,
                    syntax_errors: vec![e.to_string()],
                    missing_files: Vec::new(),
                });

                println!("⚠️  Failed to build import graph: {}", e);
                println!("   Watch mode will run all tests when files change");
            }
        }

        // Report any diagnostics
        diagnostics.report_all()?;
    } else {
        println!("✅ Using cached test collection and import graph");
    }

    // Create and start watch session
    let mut watch_session = WatchSession::new(
        work_dir.to_path_buf(),
        cache_dir.to_path_buf(),
        watch_config,
    )?;

    watch_session.start()?;

    // Set up signal handling for graceful shutdown
    setup_signal_handlers()?;

    // Run watch loop
    watch_session.run(run_options)?;

    Ok(ExitCode::Success)
}

fn setup_signal_handlers() -> Result<()> {
    // Register Ctrl+C handler
    ctrlc::set_handler(move || {
        println!("\n🛑 Stopping watch mode...");
        std::process::exit(0);
    })?;

    Ok(())
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
    _config: &Config,
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

/// Execute tests in parallel using scheduler and worker pool
fn execute_tests_parallel(
    nodeids: &[String],
    tests_index: &veri_core::python_worker::TestsIndex,
    worker_count: usize,
    run_options: &TestRunOptions,
    work_dir: &std::path::Path,
    cache_dir: &std::path::Path,
    verbose: bool,
    config: &Config,
) -> Result<ExitCode> {
    use std::time::Instant;

    println!("⚡ Scheduling tests across {} workers", worker_count);

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
    if let Ok(timings_data) = veri_core::schemas::TimingsData::load_from_cache(cache_dir) {
        if let Err(e) = scheduler.load_timings(&timings_data) {
            if verbose {
                println!("⚠️  Could not load timing data: {}", e);
            }
        } else if verbose {
            println!("📊 Loaded historical timing data");
        }
    }

    // Schedule tests into batches
    let batches = scheduler.schedule_tests(nodeids, tests_index)?;

    if batches.is_empty() {
        return Ok(ExitCode::Success);
    }

    // Show scheduling information
    let stats = scheduler.get_scheduling_stats(&batches);
    if verbose {
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
    let worker_cfg = config.worker.clone().unwrap_or_default();
    let pool_config = WorkerPoolConfig {
        worker_count,
        startup_timeout: std::time::Duration::from_secs(
            worker_cfg.startup_timeout_sec.unwrap_or(30),
        ),
        execution_timeout: std::time::Duration::from_secs(
            worker_cfg.execution_timeout_sec.unwrap_or(300),
        ),
        heartbeat_interval: std::time::Duration::from_secs(
            worker_cfg.heartbeat_interval_sec.unwrap_or(10),
        ),
        max_idle_time: std::time::Duration::from_secs(600),
        enable_recycling: true,
        work_dir: work_dir.to_path_buf(),
        cache_dir: cache_dir.to_path_buf(),
    };

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
    // Persist per-test timings for future scheduling
    if !results.is_empty() {
        use chrono::Utc;
        use std::collections::HashMap;

        // Build this run timings
        let mut test_timings: HashMap<String, veri_core::schemas::TestTiming> = HashMap::new();
        for r in &results {
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
        for r in &results {
            for t in &r.per_test {
                let entry = timings.aggregated_timings.entry(t.nodeid.clone()).or_insert(
                    veri_core::schemas::AggregatedTiming {
                        nodeid: t.nodeid.clone(),
                        run_count: 0,
                        avg_duration: 0.0,
                        min_duration: f64::MAX,
                        max_duration: 0.0,
                        p50_duration: 0.0,
                        p95_duration: 0.0,
                        last_duration: 0.0,
                        stability: 1.0,
                    },
                );
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
    }
    let total_duration = start_time.elapsed();

    // Process results
    let mut total_exit_code = 0;
    let mut failed_batches = 0;
    let mut total_tests_run = 0;

    for result in &results {
        total_tests_run += result.nodeids.len();

        if result.exit_code != 0 {
            failed_batches += 1;
            if total_exit_code == 0 {
                total_exit_code = result.exit_code;
            }
            println!("\n--- Batch failure: worker {} ({} tests, {:.1}s, exit {}) ---",
                     result.worker_id, result.nodeids.len(), result.duration.as_secs_f64(), result.exit_code);
            if !result.stdout.is_empty() {
                println!("[stdout tail]");
                for line in result.stdout.lines().rev().take(80).collect::<Vec<_>>().into_iter().rev() {
                    println!("{}", line);
                }
            }
            if !result.stderr.is_empty() {
                eprintln!("[stderr tail]");
                for line in result.stderr.lines().rev().take(80).collect::<Vec<_>>().into_iter().rev() {
                    eprintln!("{}", line);
                }
            }
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

    // Summary
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

    // Outcome rollup for parallel path
    let mut passed=0u32; let mut skipped=0u32; let mut failed=0u32; let mut error=0u32;
    for r in &results {
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
    if passed+skipped+failed+error > 0 {
        println!(
            "Summary: {} passed, {} skipped, {} failed, {} error ({} total)",
            passed, skipped, failed, error, passed+skipped+failed+error
        );
    }

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

fn init_logging(cli: &Cli) -> Result<()> {
    let log_level = if cli.verbose > 0 {
        match cli.verbose {
            1 => "INFO",
            2 => "DEBUG",
            _ => "TRACE",
        }
    } else if cli.quiet {
        "ERROR"
    } else {
        // Will be overridden by config later
        "INFO"
    };

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level)).init();

    Ok(())
}

fn handle_subcommand(command: &Commands, config: &Config, cli_args: &Cli) -> Result<ExitCode> {
    let work_dir = std::env::current_dir()?;
    let cache_dir = work_dir.join(".veri").join("cache");

    match command {
        Commands::Split { shards } => {
            eprintln!("🔀 Splitting tests into {} shards", shards);

            // Create sharder with default configuration
            let sharder_config = SharderConfig {
                strategy: veri_core::schemas::ShardingStrategy::TimingBased,
                ..Default::default()
            };
            let sharder = TestSharder::with_config(&work_dir, &cache_dir, sharder_config);

            // Generate manifest
            let manifest = sharder.split_tests(*shards, None)?;

            // Generate stats for logging
            let stats = sharder.generate_stats_summary(&manifest);

            // Output manifest to stdout as JSON
            let manifest_json = serde_json::to_string_pretty(&manifest)?;
            println!("{}", manifest_json);

            // Log statistics to stderr
            eprintln!(
                "📊 Generated {} shards with {:.1}% load balance",
                stats.total_shards,
                stats.balance_ratio * 100.0
            );
            eprintln!(
                "⏱️  Total estimated duration: {:.1}s (avg per shard: {:.1}s)",
                stats.total_estimated_duration, stats.avg_shard_duration
            );

            if cli_args.verbose > 0 {
                eprintln!("{}", sharder.format_explain(&manifest, &stats));
            }

            Ok(ExitCode::Success)
        }
        Commands::Shard { shard_id, manifest } => {
            println!("🎯 Running shard {} of CI execution", shard_id);

            // Load manifest
            let manifest_data = if let Some(manifest_path) = manifest {
                std::fs::read_to_string(manifest_path)?
            } else {
                // Read from stdin
                use std::io::Read;
                let mut buffer = String::new();
                std::io::stdin().read_to_string(&mut buffer)?;
                buffer
            };

            let manifest: veri_core::schemas::ShardsManifest =
                serde_json::from_str(&manifest_data)?;

            // Validate manifest
            let sharder = TestSharder::new(&work_dir, &cache_dir);
            sharder.validate_manifest(&manifest)?;

            // Get the specific shard
            let shard = sharder
                .get_shard(&manifest, *shard_id)?
                .ok_or_else(|| anyhow::anyhow!("Shard {} not found in manifest", shard_id))?;

            println!(
                "📋 Shard {}: {} tests, estimated {:.1}s",
                shard.shard_id, shard.test_count, shard.estimated_duration
            );

            // Extract nodeids for execution
            let nodeids = sharder.extract_nodeids(shard);

            if nodeids.is_empty() {
                println!("✅ No tests to run in this shard");
                return Ok(ExitCode::Success);
            }

            // Set up CI reporting
            let run_id = generate_run_id();
            let mut ci_reporter = CIReporter::with_shard(run_id.clone(), *shard_id);

            // Initialize output streams if configured
            if let Some(jsonl_path) = &config.jsonl {
                ci_reporter.init_jsonl(jsonl_path)?;
                if let Some(stream) = ci_reporter.event_stream() {
                    stream.emit_start(
                        manifest.shards.iter().map(|s| s.test_count).sum(),
                        shard.test_count,
                        1,
                        "shard",
                    )?;
                    stream.emit_plan(nodeids.clone(), None, shard.estimated_duration)?;
                }
            }

            if let Some(junit_path) = &config.junit_xml {
                ci_reporter.init_junit(junit_path)?;
            }

            // Execute tests for this shard
            let worker = PythonWorker::new(&work_dir, &cache_dir);

            // Configure test run options
            let run_options = veri_core::python_worker::TestRunOptions {
                verbose: cli_args.verbose > 0,
                quiet: cli_args.quiet,
                no_capture: cli_args.no_capture,
                exitfirst: cli_args.exitfirst,
                maxfail: cli_args.maxfail,
                junit_xml: config.junit_xml.clone(),
                workers: Some("1".to_string()), // Single worker for shard execution
                ignore: cli_args.ignore.clone(),
                coverage: config.cov.unwrap_or(false),
                coverage_xml: config.cov.unwrap_or(false) || config.cov_merge_full.unwrap_or(false),
                coverage_html: false,
                coverage_source_dirs: vec!["src".to_string()],
                coverage_omit: vec![
                    "*/tests/*".to_string(),
                    "*/test_*".to_string(),
                    "*/__pycache__/*".to_string(),
                    "*/venv/*".to_string(),
                    "*/.venv/*".to_string(),
                ],
            };

            println!("🚀 Executing {} tests...", nodeids.len());
            let start_time = std::time::Instant::now();

            // Run the tests
            let exec = worker.run_tests(&nodeids, &run_options)?;
            let duration = start_time.elapsed().as_secs_f64();

            // Emit summary event if JSONL enabled
            if let Some(stream) = ci_reporter.event_stream() {
                // Parse exit code to test results (simplified)
                let (passed, failed, error) = match exec.exit_code {
                    0 => (nodeids.len() as u32, 0, 0),
                    1 => (0, nodeids.len() as u32, 0), // Simplified: assume all failed
                    _ => (0, 0, nodeids.len() as u32), // Simplified: assume all error
                };

                stream.emit_summary(
                    duration,
                    nodeids.len() as u32,
                    passed,
                    failed,
                    0,
                    error,
                    exec.exit_code,
                )?;
            }

            // Finalize reporting
            ci_reporter.finalize()?;

            println!("✅ Shard {} completed in {:.1}s", shard_id, duration);

            // Return appropriate exit code
            if exec.exit_code != 0 {
                println!("\n--- Worker stdout (tail) ---");
                for line in exec.stdout.lines().rev().take(50).collect::<Vec<_>>().into_iter().rev() {
                    println!("{}", line);
                }
                eprintln!("\n--- Worker stderr (tail) ---");
                for line in exec.stderr.lines().rev().take(50).collect::<Vec<_>>().into_iter().rev() {
                    eprintln!("{}", line);
                }
            }

            match exec.exit_code {
                0 => Ok(ExitCode::Success),
                1 => Ok(ExitCode::TestFailure),
                2 => Ok(ExitCode::Interrupted),
                4 => Ok(ExitCode::UsageError),
                _ => Ok(ExitCode::InternalError),
            }
        }
    }
}

fn print_explanation(cli: &Cli, config: &Config) -> Result<()> {
    println!("=== veri Execution Plan ===");
    println!();

    // Cache key components - now with real implementation
    let config_digest = compute_config_digest(config)?;
    let cache_key = CacheKey::from_environment(config_digest)?;
    cache_key.print_explanation();
    println!();

    // Configuration summary
    println!("Configuration:");
    println!("  Engine: {}", cli.engine);
    println!("  Workers: {}", config.workers.as_deref().unwrap_or("auto"));
    println!("  Cache dir: {}", config.cache_dir().display());
    println!("  Log level: {}", config.log_level());
    println!();

    // Import graph status
    let work_dir = std::env::current_dir()?;
    let cache_dir = work_dir.join(".veri").join("cache");
    let graph_builder = ImportGraphBuilder::new(&work_dir, &cache_dir);

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
                        let worker = PythonWorker::new(&work_dir, &cache_dir);
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
