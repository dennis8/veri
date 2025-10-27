mod cli;
#[cfg(test)]
mod cli_tests;
mod commands;
mod orchestrator;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, ExitCode};
use log::info;
use orchestrator::Orchestrator;
use std::process;
use veri_core::config::Config;

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

    init_logging(&cli)?;

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

    Orchestrator::new(cli, config)?.execute()
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
        "INFO"
    };

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level)).init();

    Ok(())
}
