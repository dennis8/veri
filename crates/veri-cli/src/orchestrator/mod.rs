mod collection_service;
mod coverage;
mod execution;
mod execution_orchestrator;
mod selection;
mod selection_orchestrator;
pub mod services;
mod telemetry;
mod telemetry_orchestrator;
mod validation_service;
mod vcs;
mod watch;
mod workspace;

use crate::cli::{Cli, Engine, ExitCode};
use crate::commands;
use anyhow::Result;
use log::warn;
use telemetry::TelemetryService;
use veri_core::compatibility::CompatibilityMatrix;
use veri_core::config::Config;
use veri_core::flaky::{FlakyConfig, FlakyManager};
use veri_core::security::SecurityConfig;
use watch::{DefaultWatchAdapter, WatchAdapter};

pub struct Orchestrator {
    cli: Cli,
    config: Config,
    security_config: SecurityConfig,
    compatibility_matrix: CompatibilityMatrix,
    telemetry: TelemetryService,
    flaky_manager: FlakyManager,
    watch_adapter: Box<dyn WatchAdapter>,
}

impl Orchestrator {
    pub fn new(cli: Cli, config: Config) -> Result<Self> {
        let work_dir = std::env::current_dir()?;
        let cache_dir = work_dir.join(".veri").join("cache");

        let security_config = SecurityConfig::from_config(&config);
        let compatibility_matrix =
            CompatibilityMatrix::load_or_default(work_dir.join("veri-compatibility.toml"))
                .unwrap_or_default();

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

        let telemetry = TelemetryService::new(&config);

        Ok(Self {
            cli,
            config,
            security_config,
            compatibility_matrix,
            telemetry,
            flaky_manager,
            watch_adapter: Box::new(DefaultWatchAdapter),
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn with_watch_adapter(mut self, adapter: Box<dyn WatchAdapter>) -> Self {
        self.watch_adapter = adapter;
        self
    }

    pub fn execute(mut self) -> Result<ExitCode> {
        if self.cli.flaky_report {
            self.print_flaky_report();
            return Ok(ExitCode::Success);
        }

        if self.cli.telemetry_status {
            self.telemetry.print_status(!self.config.no_color());
            return Ok(ExitCode::Success);
        }

        if let Some(command) = &self.cli.command {
            return commands::handle_subcommand(command, &self.config, &self.cli);
        }

        if self.cli.explain {
            print_explanation(&self.cli, &self.config)?;
            if self.cli.engine == Engine::Pytest {
                println!("Note: Using --engine pytest - no impact analysis will be performed");
            }
            return Ok(ExitCode::Success);
        }

        match self.cli.engine {
            Engine::Pytest => run_pytest_engine(&self.cli, &self.config),
            Engine::Veri => run_veri_engine(
                &self.cli,
                &self.config,
                &self.security_config,
                &self.compatibility_matrix,
                &mut self.telemetry,
                self.watch_adapter.as_ref(),
            ),
        }
    }

    fn print_flaky_report(&mut self) {
        println!("📊 Flaky Test Report");
        println!("====================");
        let report = self.flaky_manager.generate_report();
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
    }
}

mod internals;
pub use internals::{execute_tests_parallel, ParallelExecutionConfig};
use internals::{print_explanation, run_pytest_engine, run_veri_engine};

#[cfg(test)]
mod orchestrator_tests {
    use super::*;
    use anyhow::Result;
    use clap::Parser;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    struct TestWatchAdapter {
        invoked: Arc<AtomicBool>,
        exit: ExitCode,
    }

    impl WatchAdapter for TestWatchAdapter {
        fn run(
            &self,
            _cli: &Cli,
            _work_dir: &Path,
            _cache_dir: &Path,
            _runtime: &veri_core::python_launcher::PythonRuntime,
        ) -> Result<ExitCode> {
            self.invoked.store(true, Ordering::SeqCst);
            Ok(self.exit)
        }
    }

    #[test]
    fn orchestrator_uses_custom_watch_adapter() {
        let cli = Cli::parse_from(["veri", "--watch"]);
        let config = Config::default();

        let invoked = Arc::new(AtomicBool::new(false));
        let adapter = TestWatchAdapter {
            invoked: Arc::clone(&invoked),
            exit: ExitCode::Success,
        };

        let orchestrator = Orchestrator::new(cli, config)
            .expect("construct orchestrator")
            .with_watch_adapter(Box::new(adapter));

        let exit = orchestrator.execute().expect("execute orchestrator");

        assert!(invoked.load(Ordering::SeqCst));
        assert_eq!(exit, ExitCode::Success);
    }

    #[test]
    fn orchestrator_handles_telemetry_status_short_circuit() {
        let cli = Cli::parse_from(["veri", "--telemetry-status"]);
        let config = Config::default();

        let orchestrator = Orchestrator::new(cli, config).expect("construct orchestrator");
        let exit = orchestrator.execute().expect("execute orchestrator");

        assert_eq!(exit, ExitCode::Success);
    }
}
