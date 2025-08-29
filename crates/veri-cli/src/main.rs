mod cli;
#[cfg(test)]
mod cli_tests;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands, ExitCode};
use log::info;
use std::process;
use veri_core::config::Config;
use veri_core::cache::{CacheKey, compute_config_digest};

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
    }
    
    // For now, just print what we would do (Phase 1 - no business logic yet)
    print_planned_execution(&cli, &config)?;
    
    // Return success for now (Phase 1)
    Ok(ExitCode::Success)
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
    
    // Selection logic (placeholder)
    if cli.all {
        println!("Selection: Running ALL tests (--all specified)");
    } else if cli.last_failed {
        println!("Selection: Running last failed tests");
    } else if cli.keyword.is_some() || cli.marker.is_some() {
        if let Some(keyword) = &cli.keyword {
            println!("Selection: Keyword filter: '{}'", keyword);
        }
        if let Some(marker) = &cli.marker {
            println!("Selection: Marker filter: '{}'", marker);
        }
    } else {
        println!("Selection: Impact-aware (based on changed files)");
        println!("  Changed files: (none detected - would analyze git/fs)");
        println!("  Impacted tests: (would compute from import graph)");
    }
    
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
