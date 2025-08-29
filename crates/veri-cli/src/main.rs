mod cli;
#[cfg(test)]
mod cli_tests;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands, ExitCode, Engine};
use log::info;
use std::process;
use veri_core::config::Config;
use veri_core::cache::{CacheKey, compute_config_digest};
use veri_core::python_worker::{PythonWorker, TestRunOptions};
use veri_core::import_graph::ImportGraphBuilder;
use veri_core::planner::{TestPlanner, PlannerConfig};

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
    );
    
    info!("veri v{} starting", env!("CARGO_PKG_VERSION"));
    
    // Handle version flag (clap handles this automatically with --version)
    // Handle help flag (clap handles this automatically with --help)
    
    // Handle subcommands first
    if let Some(command) = &cli.command {
        return handle_subcommand(command, &config);
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
            run_veri_engine(&cli, &config)
        }
    }
}

fn run_pytest_engine(cli: &Cli, _config: &Config) -> Result<ExitCode> {
    println!("🔄 Using pytest engine for compatibility");
    
    // Create Python worker
    let work_dir = std::env::current_dir()?;
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
    
    // Add paths
    pytest_args.extend(cli.paths.clone());
    
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

fn run_veri_engine(cli: &Cli, config: &Config) -> Result<ExitCode> {
    println!("🚀 Using veri engine for maximum speed");
    
    let work_dir = std::env::current_dir()?;
    let cache_dir = work_dir.join(".veri").join("cache");
    let worker = PythonWorker::new(&work_dir, &cache_dir);
    
    // Check if we need to collect tests (first run or --all)
    let needs_collection = cli.all || !worker.has_valid_cache();
    
    if needs_collection {
        println!("📋 Collecting tests...");
        
        // Determine paths to collect
        let collection_paths = if !cli.paths.is_empty() {
            cli.paths.clone()
        } else {
            vec![] // Empty means collect all
        };
        
        // Collect tests
        let tests_index = worker.collect_tests(&collection_paths)?;
        
        println!("✅ Collected {} tests", tests_index.tests.len());
        
        if !tests_index.collection_errors.is_empty() {
            println!("⚠️  {} collection errors encountered", tests_index.collection_errors.len());
            for error in &tests_index.collection_errors {
                eprintln!("  {}: {}", error.path, error.message);
            }
        }
        
        // Build import graph and dependency analysis
        println!("🔍 Building import graph...");
        let mut graph_builder = ImportGraphBuilder::new(&work_dir, &cache_dir);
        let (imports_graph, revdeps_graph, module_map) = graph_builder.build_graphs()?;
        
        println!("✅ Built import graph with {} edges", imports_graph.edges.len());
        if !imports_graph.dynamic_imports.is_empty() {
            println!("⚠️  {} dynamic imports detected", imports_graph.dynamic_imports.len());
        }
        
        // If this was just collection (--all with no other action), we're done
        if cli.all && cli.paths.is_empty() && !should_run_tests(cli) {
            return Ok(ExitCode::Success);
        }
    }
    
    // Load collected tests and graphs
    let tests_index = worker.collect_tests(&cli.paths)?;
    
    // Load or build graphs
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
    
    // Determine which tests to run using impact analysis
    let nodeids_to_run = select_tests_to_run(
        &tests_index, 
        &imports_graph,
        &revdeps_graph,
        &module_map,
        cli, 
        config
    )?;
    
    if nodeids_to_run.is_empty() {
        println!("🎯 No tests selected to run");
        return Ok(ExitCode::Success);
    }
    
    println!("🎯 Running {} selected tests", nodeids_to_run.len());
    
    // Configure test run options
    let run_options = TestRunOptions {
        verbose: cli.verbose > 0,
        quiet: cli.quiet,
        no_capture: cli.no_capture,
        exitfirst: cli.exitfirst,
        maxfail: cli.maxfail,
        junit_xml: cli.junit_xml.clone(),
        workers: cli.workers.clone(),
    };
    
    // Execute tests
    let exit_code = worker.run_tests(&nodeids_to_run, &run_options)?;
    
    match exit_code {
        0 => Ok(ExitCode::Success),
        1 => Ok(ExitCode::TestFailure),
        2 => Ok(ExitCode::Interrupted),
        4 => Ok(ExitCode::UsageError),
        _ => Ok(ExitCode::InternalError),
    }
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
            include &= cli.paths.iter().any(|path| {
                test.path.starts_with(path) || test.nodeid.contains(path)
            });
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
    
    // Print selection summary
    if selection.should_broaden {
        println!("⚠️  Selection broadened: {}", selection.broaden_reason.as_deref().unwrap_or("Unknown"));
    }
    
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
        .args(&["rev-parse", "--show-toplevel"])
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
        .args(&["diff", "--name-only", "HEAD"])
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
                        std::path::Path::new(line).file_name()
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
    
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level))
        .init();
    
    Ok(())
}

fn handle_subcommand(command: &Commands, _config: &Config) -> Result<ExitCode> {
    match command {
        Commands::Split { shards } => {
            println!("Would split tests into {} shards", shards);
            // TODO: Implement actual splitting logic in later phases
            Ok(ExitCode::Success)
        }
        Commands::Shard { shard_id, manifest } => {
            println!("Would run shard {} from manifest {:?}", shard_id, manifest);
            // TODO: Implement actual shard execution in later phases
            Ok(ExitCode::Success)
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
        Some((imports_graph, revdeps_graph, module_map)) => {
            println!("  Status: Cached graphs available");
            println!("  Modules: {}", module_map.modules.len());
            println!("  Import edges: {}", imports_graph.edges.len());
            println!("  Dynamic imports: {}", imports_graph.dynamic_imports.len());
            println!("  Unresolved imports: {}", imports_graph.unresolved_imports.len());
            
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
                    if let Ok(Some((imports_graph, revdeps_graph, module_map))) = graph_builder.load_cached_graphs() {
                        let worker = PythonWorker::new(&work_dir, &cache_dir);
                        if worker.has_valid_cache() {
                            if let Ok(tests_index) = worker.collect_tests(&[]) {
                                let planner = TestPlanner::new(&work_dir, &cache_dir);
                                if let Ok(selection) = planner.plan_test_selection(
                                    &changed_files,
                                    &tests_index,
                                    &revdeps_graph,
                                    &module_map,
                                    &imports_graph,
                                ) {
                                    println!("  Impact Analysis:");
                                    println!("    Selected tests: {} of {}", 
                                        selection.selected_nodeids.len(), 
                                        selection.total_tests);
                                    if selection.should_broaden {
                                        println!("    ⚠️  Broadened: {}", 
                                            selection.broaden_reason.as_deref().unwrap_or("Unknown"));
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
    
    Ok(())
}

fn print_planned_execution(cli: &Cli, config: &Config) -> Result<()> {
    println!("veri v{} - ultra-fast pytest-compatible test runner", env!("CARGO_PKG_VERSION"));
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
    println!("  1. Load configuration from: {}", 
        cli.config.as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "veri.toml or pyproject.toml".to_string()));
    
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
