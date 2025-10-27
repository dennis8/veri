use anyhow::Result;
use veri_core::cache::{compute_config_digest, CacheKey};
use veri_core::config::Config;
use veri_core::python_launcher::PythonRuntime;
use veri_core::telemetry::{RunEvent, ErrorCategory};

use crate::cli::Cli;
use super::execution::ExecutionResult;
use super::telemetry::TelemetryService;

/// Trait for orchestrating telemetry recording
pub trait TelemetryOrchestrationService: Send + Sync {
    fn record_execution_metrics(
        &self,
        execution_result: &ExecutionResult,
        collection_time_ms: u64,
        cli: &Cli,
        config: &Config,
        telemetry: &mut TelemetryService,
        needs_collection: bool,
    ) -> Result<()>;
}

/// Default implementation for telemetry recording
pub struct DefaultTelemetryOrchestrator {
    python_runtime: PythonRuntime,
}

impl DefaultTelemetryOrchestrator {
    pub fn new(python_runtime: PythonRuntime) -> Self {
        Self { python_runtime }
    }
}

impl TelemetryOrchestrationService for DefaultTelemetryOrchestrator {
    fn record_execution_metrics(
        &self,
        execution_result: &ExecutionResult,
        collection_time_ms: u64,
        cli: &Cli,
        config: &Config,
        telemetry: &mut TelemetryService,
        needs_collection: bool,
    ) -> Result<()> {
        // Get actual Python version from cache key
        let config_digest = compute_config_digest(config)?;
        let cache_key = CacheKey::from_environment(config_digest, Some(&self.python_runtime.launcher))?;
        let python_version = Some(cache_key.python_version);

        // Record telemetry for this run
        let run_event = RunEvent {
            test_count: 0, // Will be set by caller if needed
            worker_count: parse_worker_count(&cli.workers)? as u32,
            collection_time_ms,
            execution_time_ms: execution_result.duration.as_millis() as u64,
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
                if parse_worker_count(&cli.workers)? > 1 {
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
        if execution_result.exit_code != crate::cli::ExitCode::Success
            && execution_result.exit_code != crate::cli::ExitCode::TestFailure
        {
            telemetry.record_error(ErrorCategory::ExecutionError);
        }

        Ok(())
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

#[cfg(test)]
mod testing {
    use super::*;

    /// Mock telemetry orchestrator for testing
    pub struct MockTelemetryOrchestrator;

    impl MockTelemetryOrchestrator {
        pub fn new() -> Self {
            Self
        }
    }

    impl TelemetryOrchestrationService for MockTelemetryOrchestrator {
        fn record_execution_metrics(
            &self,
            _execution_result: &ExecutionResult,
            _collection_time_ms: u64,
            _cli: &Cli,
            _config: &Config,
            _telemetry: &mut TelemetryService,
            _needs_collection: bool,
        ) -> Result<()> {
            Ok(())
        }
    }
}
