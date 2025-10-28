use super::execution::ExecutionService;
use super::services::OrchestratorServices;
use super::telemetry::TelemetryService;
use super::watch::WatchAdapter;
use super::workspace::WorkspaceService;
use super::vcs::VcsService;
use super::validation_service::{ValidationOrchestrationService, DefaultValidationService};
use super::collection_service::{CollectionOrchestrationService, DefaultCollectionService};
use super::selection_orchestrator::{SelectionOrchestrationService, DefaultSelectionOrchestrator};
use super::execution_orchestrator::{ExecutionOrchestrationService, DefaultExecutionOrchestrator};
use super::telemetry_orchestrator::{TelemetryOrchestrationService, DefaultTelemetryOrchestrator};
use crate::cli::{Cli, ExitCode};
use anyhow::Result;
use veri_core::cache::{compute_config_digest, CacheKey};
use veri_core::compatibility::CompatibilityMatrix;
use veri_core::config::Config;
use veri_core::diagnostics::{DiagnosticReporter, VeriDiagnostic};
use veri_core::import_graph::ImportGraphBuilder;
use veri_core::planner::TestPlanner;
use veri_core::python_launcher::PythonRuntime;
use veri_core::python_worker::{PythonWorker, TestRunOptions};
use veri_core::security::{SecurityConfig, SecurityScanner};
use veri_core::telemetry::ErrorCategory;

pub(super) fn run_pytest_engine(cli: &Cli, config: &Config) -> Result<ExitCode> {
    println!("🔄 Using pytest engine for compatibility");

    // Create Python worker
    // Initialize workspace service to detect project root
    let initial_work_dir = std::env::current_dir()?;
    let workspace = super::workspace::FilesystemWorkspaceService::new(&initial_work_dir, &initial_work_dir);
    let work_dir = workspace.detect_project_root(&cli.paths).unwrap_or(initial_work_dir);
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
    let norm_paths = workspace.normalize_paths(&cli.paths);
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

    // ===== SETUP PHASE =====
    let (work_dir, cache_dir, services, python_runtime, needs_collection) =
        setup_orchestration(cli, config)?;

    // Handle watch mode
    if cli.watch {
        return watch_adapter.run(cli, &work_dir, &cache_dir, &python_runtime);
    }

    let worker = PythonWorker::from_runtime(&work_dir, &cache_dir, &python_runtime);
    let mut diagnostics = DiagnosticReporter::new(cli.quiet);

    // ===== STAGE 1: VALIDATION =====
    let validation_svc = DefaultValidationService::new();
    let validation = validation_svc.validate_environment(
        cli,
        config,
        &worker,
        compatibility_matrix,
        security_config,
        telemetry,
        &mut diagnostics,
    )?;
    if let Some(exit) = validation.fallback_exit {
        return Ok(exit);
    }

    // ===== STAGE 2: COLLECTION =====
    let collection_paths = if !cli.paths.is_empty() {
        services.workspace.normalize_paths(&cli.paths)
    } else {
        vec![]
    };

    let collection_svc = DefaultCollectionService::new(&work_dir, &cache_dir, python_runtime.clone());
    let collection = collection_svc.collect_or_load(
        cli.all,
        &collection_paths,
        &cli.ignore,
        &mut diagnostics,
    )?;

    // If this was just collection (--all with no other action), we're done
    if cli.all && cli.paths.is_empty() && !should_run_tests(cli) {
        return Ok(ExitCode::Success);
    }

    // Report any diagnostics from collection phase
    diagnostics.report_all()?;
    if diagnostics.has_errors() {
        return Ok(ExitCode::InternalError);
    }

    let tests_index = collection.tests_index;
    let collection_time_ms = collection.collection_time_ms;
    let graphs = collection.graphs;

    // ===== STAGE 3: SELECTION =====
    let selection_svc = DefaultSelectionOrchestrator::new(&work_dir, &cache_dir, python_runtime.clone());
    let selection = selection_svc.determine_tests_to_run(
        &tests_index,
        cli,
        &services,
        graphs.as_ref().map(|(ig, rg, mm)| (ig, rg, mm)),
    )?;

    if let Some(exit) = selection.early_exit {
        if exit == ExitCode::UsageError {
            // Print diagnostic for no tests found
            let mut diag = DiagnosticReporter::new(cli.quiet);
            diag.add(VeriDiagnostic::no_tests_found(
                cli.keyword.as_deref(),
                cli.marker.as_deref(),
                &cli.paths,
            ));
            diag.report_all()?;
        }
        return Ok(exit);
    }

    let nodeids_to_run = selection.nodeids;
    println!("🎯 Running {} selected tests", nodeids_to_run.len());

    // ===== STAGE 4: EXECUTION =====
    let execution_svc = DefaultExecutionOrchestrator::new(&work_dir, &cache_dir);
    let execution = execution_svc.execute_with_coverage(&nodeids_to_run, &tests_index, cli, &services)?;

    // ===== STAGE 5: TELEMETRY =====
    let telemetry_svc = DefaultTelemetryOrchestrator::new(python_runtime);
    telemetry_svc.record_execution_metrics(
        &execution.result,
        collection_time_ms,
        cli,
        config,
        telemetry,
        needs_collection,
    )?;

    Ok(execution.result.exit_code)
}

/// Helper function to set up orchestration environment
fn setup_orchestration(
    cli: &Cli,
    config: &Config,
) -> Result<(
    std::path::PathBuf,
    std::path::PathBuf,
    OrchestratorServices,
    PythonRuntime,
    bool,
)> {
    let initial_work_dir = std::env::current_dir()?;
    let temp_workspace = super::workspace::FilesystemWorkspaceService::new(&initial_work_dir, &initial_work_dir);
    let work_dir = temp_workspace.detect_project_root(&cli.paths).unwrap_or(initial_work_dir);
    let work_dir = std::fs::canonicalize(&work_dir).unwrap_or(work_dir);
    let cache_dir = work_dir.join(".veri").join("cache");
    let python_runtime_cfg = config.python();
    let python_runtime = PythonRuntime::from_config(&work_dir, &python_runtime_cfg);

    let services = OrchestratorServices::new_default(
        &work_dir,
        &cache_dir,
        cli.verbose > 0,
        config,
        &python_runtime,
    );

    // Check if we need to collect tests (first run or --all)
    let temp_worker = PythonWorker::from_runtime(&work_dir, &cache_dir, &python_runtime);
    let needs_collection = cli.all || !temp_worker.has_valid_cache();

    Ok((work_dir, cache_dir, services, python_runtime, needs_collection))
}

fn should_run_tests(cli: &Cli) -> bool {
    // Check if any flags indicate we should actually run tests, not just collect
    cli.keyword.is_some()
        || cli.marker.is_some()
        || cli.last_failed
        || !cli.paths.is_empty()
        || cli.watch
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
/// DEPRECATED: Use ExecutionService instead
pub struct ParallelExecutionConfig<'a> {
    pub work_dir: &'a std::path::Path,
    pub cache_dir: &'a std::path::Path,
    pub verbose: bool,
    pub config: &'a Config,
    pub python_runtime: PythonRuntime,
}

/// DEPRECATED: Use ExecutionService::execute_tests instead
/// Kept for backward compatibility during beta
pub fn execute_tests_parallel(
    nodeids: &[String],
    tests_index: &veri_core::python_worker::TestsIndex,
    _worker_count: usize,
    run_options: &TestRunOptions,
    exec_config: &ParallelExecutionConfig,
) -> Result<ExitCode> {
    // Delegate to the new ExecutionService
    let service = super::execution::ParallelExecutionService::new(
        exec_config.work_dir,
        exec_config.cache_dir,
        exec_config.verbose,
        exec_config.config,
        exec_config.python_runtime.clone(),
    );
    let result = service.execute_tests(nodeids, tests_index, run_options)?;
    Ok(result.exit_code)
}

pub(super) fn print_explanation(cli: &Cli, config: &Config) -> Result<()> {
    println!("=== veri Execution Plan ===");
    println!();

    // Import graph status
    let work_dir = std::env::current_dir()?;
    let cache_dir = work_dir.join(".veri").join("cache");
    let python_runtime_cfg = config.python();
    let python_runtime = PythonRuntime::from_config(&work_dir, &python_runtime_cfg);

    // Initialize VCS service for getting changed files
    let vcs_service = super::vcs::GitVcsService::new(&work_dir);

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
        match vcs_service.get_changed_files() {
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
