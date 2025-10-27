use anyhow::Result;
use veri_core::python_worker::{TestsIndex, TestRunOptions};
use std::path::Path;

use crate::cli::Cli;
use super::coverage::CoverageService;
use super::execution::ExecutionResult;
use super::services::OrchestratorServices;

/// Result of test execution operation
#[derive(Debug, Clone)]
pub struct ExecutionOutcome {
    /// Execution result
    pub result: ExecutionResult,
    /// Coverage data if collected
    pub coverage_collected: bool,
}

/// Trait for orchestrating test execution
pub trait ExecutionOrchestrationService: Send + Sync {
    fn execute_with_coverage(
        &self,
        nodeids: &[String],
        tests_index: &TestsIndex,
        cli: &Cli,
        services: &OrchestratorServices,
    ) -> Result<ExecutionOutcome>;
}

/// Default implementation for test execution
pub struct DefaultExecutionOrchestrator {
    work_dir: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
}

impl DefaultExecutionOrchestrator {
    pub fn new(work_dir: &Path, cache_dir: &Path) -> Self {
        Self {
            work_dir: work_dir.to_path_buf(),
            cache_dir: cache_dir.to_path_buf(),
        }
    }
}

impl ExecutionOrchestrationService for DefaultExecutionOrchestrator {
    fn execute_with_coverage(
        &self,
        nodeids: &[String],
        tests_index: &TestsIndex,
        cli: &Cli,
        services: &OrchestratorServices,
    ) -> Result<ExecutionOutcome> {
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

        // Initialize coverage
        let coverage_service = CoverageService::new(cli, &self.work_dir, &self.cache_dir);
        coverage_service.initialize(nodeids)?;

        // Execute tests using the execution service
        let execution_result =
            services
                .execution
                .execute_tests(nodeids, tests_index, &run_options)?;

        // Finalize coverage
        coverage_service.finalize(cli, nodeids)?;

        Ok(ExecutionOutcome {
            result: execution_result,
            coverage_collected: cli.cov || cli.cov_merge_full,
        })
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
    use std::sync::Arc;
    use std::sync::Mutex;

    /// Mock execution orchestrator for testing
    pub struct MockExecutionOrchestrator {
        pub outcome: Arc<Mutex<ExecutionOutcome>>,
    }

    impl MockExecutionOrchestrator {
        pub fn new(outcome: ExecutionOutcome) -> Self {
            Self {
                outcome: Arc::new(Mutex::new(outcome)),
            }
        }

        pub fn with_success() -> Self {
            use std::time::Duration;
            use crate::cli::ExitCode;
            Self::new(ExecutionOutcome {
                result: ExecutionResult {
                    exit_code: ExitCode::Success,
                    duration: Duration::from_secs(1),
                    total_tests_run: 0,
                    passed: 0,
                    failed: 0,
                    skipped: 0,
                    error: 0,
                },
                coverage_collected: false,
            })
        }

        pub fn with_failure() -> Self {
            use std::time::Duration;
            use crate::cli::ExitCode;
            Self::new(ExecutionOutcome {
                result: ExecutionResult {
                    exit_code: ExitCode::TestFailure,
                    duration: Duration::from_secs(1),
                    total_tests_run: 0,
                    passed: 0,
                    failed: 1,
                    skipped: 0,
                    error: 0,
                },
                coverage_collected: false,
            })
        }
    }

    impl ExecutionOrchestrationService for MockExecutionOrchestrator {
        fn execute_with_coverage(
            &self,
            _nodeids: &[String],
            _tests_index: &TestsIndex,
            _cli: &Cli,
            _services: &OrchestratorServices,
        ) -> Result<ExecutionOutcome> {
            Ok(self
                .outcome
                .lock()
                .map_err(|e| anyhow::anyhow!("Mock lock failed: {}", e))?
                .clone())
        }
    }
}
