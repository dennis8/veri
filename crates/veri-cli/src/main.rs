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
use veri_core::scheduler::{TestScheduler, SchedulerConfig, SchedulingStrategy};
use veri_core::worker_pool::{WorkerPool, WorkerPoolConfig};
use veri_core::coverage::{CoverageCollector, CoverageConfig, CoverageFormat};
use veri_core::watch::{WatchSession, WatchConfig};
use veri_core::sharder::{TestSharder, SharderConfig};
use veri_core::event_stream::{CIReporter, generate_run_id};

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
    
    // Handle watch mode
    if cli.watch {
        return run_watch_mode(cli, config, &work_dir, &cache_dir);
    }
    
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
    
    // Parse worker count
    let worker_count = parse_worker_count(&cli.workers)?;
    
    // Configure test run options
    let run_options = TestRunOptions {
        verbose: cli.verbose > 0,
        quiet: cli.quiet,
        no_capture: cli.no_capture,
        exitfirst: cli.exitfirst,
        maxfail: cli.maxfail,
        junit_xml: cli.junit_xml.clone(),
        workers: Some("1".to_string()), // Each worker handles their batch sequentially
        coverage: cli.cov,
        coverage_xml: cli.cov || cli.cov_merge_full,
        coverage_html: false, // Can be configured later
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

    let coverage_collector = coverage_config.as_ref().map(|config| {
        CoverageCollector::new(config.clone(), cache_dir.clone(), work_dir.clone())
    });

    // Initialize coverage for selected tests
    if let Some(collector) = &coverage_collector {
        collector.initialize_coverage(&nodeids_to_run)?;
    }

    // Execute tests using scheduler and worker pool if we have multiple workers
    let test_result = if worker_count == 1 || nodeids_to_run.len() == 1 {
        // Single worker execution - use direct approach
        let exit_code = worker.run_tests(&nodeids_to_run, &run_options)?;
        match exit_code {
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
        )
    };

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

fn run_watch_mode(cli: &Cli, config: &Config, work_dir: &std::path::Path, cache_dir: &std::path::Path) -> Result<ExitCode> {
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
        match worker.collect_tests(&[]) {
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
        match graph_builder.build_graphs() {
            Ok((imports_graph, _revdeps_graph, _module_map)) => {
                println!("✅ Built import graph with {} edges", imports_graph.edges.len());
            }
            Err(e) => {
                println!("⚠️  Failed to build import graph: {}", e);
                println!("   Watch mode will run all tests when files change");
            }
        }
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
        println!("📋 Scheduled {} tests across {} workers", stats.total_tests, stats.total_workers);
        println!("⏱️  Estimated duration: {:.1}s (load balance: {:.1}%)", 
            stats.total_estimated_duration_ms as f64 / 1000.0,
            stats.load_balance_ratio * 100.0);
    }
    
    // Configure worker pool
    let pool_config = WorkerPoolConfig {
        worker_count,
        startup_timeout: std::time::Duration::from_secs(30),
        execution_timeout: std::time::Duration::from_secs(300),
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
        }
        
        if verbose {
            println!("Worker {} completed {} tests in {:.1}s (exit: {})",
                result.worker_id,
                result.nodeids.len(),
                result.duration.as_secs_f64(),
                result.exit_code);
        }
    }
    
    // Summary
    println!("✅ Completed {} tests in {:.1}s using {} workers",
        total_tests_run,
        total_duration.as_secs_f64(),
        worker_count);
    
    if failed_batches > 0 {
        println!("❌ {} of {} worker batches reported failures", failed_batches, results.len());
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
    
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level))
        .init();
    
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
            eprintln!("📊 Generated {} shards with {:.1}% load balance", 
                stats.total_shards, stats.balance_ratio * 100.0);
            eprintln!("⏱️  Total estimated duration: {:.1}s (avg per shard: {:.1}s)", 
                stats.total_estimated_duration, stats.avg_shard_duration);
            
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
            
            let manifest: veri_core::schemas::ShardsManifest = serde_json::from_str(&manifest_data)?;
            
            // Validate manifest
            let sharder = TestSharder::new(&work_dir, &cache_dir);
            sharder.validate_manifest(&manifest)?;
            
            // Get the specific shard
            let shard = sharder.get_shard(&manifest, *shard_id)?
                .ok_or_else(|| anyhow::anyhow!("Shard {} not found in manifest", shard_id))?;
            
            println!("📋 Shard {}: {} tests, estimated {:.1}s", 
                shard.shard_id, shard.test_count, shard.estimated_duration);
            
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
                    stream.emit_start(manifest.shards.iter().map(|s| s.test_count).sum(), shard.test_count, 1, "shard")?;
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
            let exit_code = worker.run_tests(&nodeids, &run_options)?;
            let duration = start_time.elapsed().as_secs_f64();
            
            // Emit summary event if JSONL enabled
            if let Some(stream) = ci_reporter.event_stream() {
                // Parse exit code to test results (simplified)
                let (passed, failed, error) = match exit_code {
                    0 => (nodeids.len() as u32, 0, 0),
                    1 => (0, nodeids.len() as u32, 0), // Simplified: assume all failed
                    _ => (0, 0, nodeids.len() as u32), // Simplified: assume all error
                };
                
                stream.emit_summary(duration, nodeids.len() as u32, passed, failed, 0, error, exit_code)?;
            }
            
            // Finalize reporting
            ci_reporter.finalize()?;
            
            println!("✅ Shard {} completed in {:.1}s", shard_id, duration);
            
            // Return appropriate exit code
            match exit_code {
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
