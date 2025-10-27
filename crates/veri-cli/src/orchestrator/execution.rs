use crate::cli::ExitCode;
use anyhow::Result;
use chrono::Utc;
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};
use veri_core::config::Config;
use veri_core::python_launcher::PythonRuntime;
use veri_core::python_worker::{TestRunOptions, TestsIndex};
use veri_core::scheduler::{SchedulerConfig, SchedulingStrategy, TestScheduler};
use veri_core::worker_pool::{WorkerPool, WorkerPoolConfig};

/// Configuration for parallel test execution
#[allow(dead_code)]
pub struct ExecutionConfig<'a> {
    pub work_dir: &'a Path,
    pub cache_dir: &'a Path,
    pub verbose: bool,
    pub config: &'a Config,
    pub python_runtime: PythonRuntime,
}

/// Results from test execution
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct ExecutionResult {
    pub exit_code: ExitCode,
    pub duration: Duration,
    pub total_tests_run: usize,
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub error: u32,
}

/// Service trait for test execution
#[allow(dead_code)]
pub trait ExecutionService: Send + Sync {
    /// Execute tests and return results
    fn execute_tests(
        &self,
        nodeids: &[String],
        tests_index: &TestsIndex,
        run_options: &TestRunOptions,
    ) -> Result<ExecutionResult>;
}

/// Default implementation using worker pool and scheduler
pub struct ParallelExecutionService {
    config: Config,
    python_runtime: PythonRuntime,
    work_dir: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
    verbose: bool,
}

impl ParallelExecutionService {
    pub fn new(
        work_dir: &Path,
        cache_dir: &Path,
        verbose: bool,
        config: &Config,
        python_runtime: PythonRuntime,
    ) -> Self {
        Self {
            config: config.clone(),
            python_runtime,
            work_dir: work_dir.to_path_buf(),
            cache_dir: cache_dir.to_path_buf(),
            verbose,
        }
    }
}

impl ExecutionService for ParallelExecutionService {
    fn execute_tests(
        &self,
        nodeids: &[String],
        tests_index: &TestsIndex,
        run_options: &TestRunOptions,
    ) -> Result<ExecutionResult> {
        let start_time = Instant::now();

        // Parse worker count
        let worker_count = parse_worker_count(&run_options.workers)?;

        println!("⚡ Scheduling tests across {} workers", worker_count);

        // Setup scheduler and create test batches
        let (scheduler, batches) = setup_scheduler(
            worker_count,
            nodeids,
            tests_index,
            &self.work_dir,
            &self.cache_dir,
            self.verbose,
        )?;

        if batches.is_empty() {
            return Ok(ExecutionResult {
                exit_code: ExitCode::Success,
                duration: start_time.elapsed(),
                total_tests_run: 0,
                passed: 0,
                failed: 0,
                skipped: 0,
                error: 0,
            });
        }

        // Show scheduling information
        let stats = scheduler.get_scheduling_stats(&batches);
        if self.verbose {
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
        let worker_cfg = self.config.worker.clone().unwrap_or_default();
        let mut pool_config = WorkerPoolConfig {
            worker_count,
            startup_timeout: Duration::from_secs(worker_cfg.startup_timeout_sec.unwrap_or(30)),
            execution_timeout: Duration::from_secs(worker_cfg.execution_timeout_sec.unwrap_or(300)),
            heartbeat_interval: Duration::from_secs(
                worker_cfg.heartbeat_interval_sec.unwrap_or(10),
            ),
            work_dir: self.work_dir.clone(),
            cache_dir: self.cache_dir.clone(),
            ..Default::default()
        };
        pool_config.apply_runtime(&self.python_runtime);

        // Create and start worker pool
        let mut worker_pool = WorkerPool::new(pool_config);
        worker_pool.start()?;

        // Submit batches to worker pool
        for (i, batch) in batches.iter().enumerate() {
            let batch_id = format!("batch_{}", i);
            worker_pool.submit_batch(batch_id, batch.clone(), run_options.clone())?;
        }

        println!("🚀 Executing tests...");

        // Wait for completion
        let results = worker_pool.wait_for_completion(Some(Duration::from_secs(600)))?;
        let total_duration = start_time.elapsed();

        // Persist timing data for future scheduling optimization
        persist_timing_data(&results, worker_count, &self.cache_dir)?;

        // Process results and extract statistics
        let (total_exit_code, _failed_batches, total_tests_run, passed, failed, skipped, error) =
            process_and_summarize_results(&results, total_duration, worker_count, self.verbose);

        // Shutdown worker pool
        worker_pool.shutdown()?;

        // Map exit code to ExitCode enum
        let exit_code = match total_exit_code {
            0 => ExitCode::Success,
            1 => ExitCode::TestFailure,
            2 => ExitCode::Interrupted,
            4 => ExitCode::UsageError,
            _ => ExitCode::InternalError,
        };

        Ok(ExecutionResult {
            exit_code,
            duration: total_duration,
            total_tests_run,
            passed,
            failed,
            skipped,
            error,
        })
    }
}

// ============================================================================
// Private helper functions
// ============================================================================

/// Parse worker count from optional string
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

/// Setup test scheduler with historical timing data
fn setup_scheduler(
    worker_count: usize,
    nodeids: &[String],
    tests_index: &TestsIndex,
    _work_dir: &Path,
    cache_dir: &Path,
    verbose: bool,
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

    Ok((scheduler, batches))
}

/// Persist test timing data for future scheduling optimization
fn persist_timing_data(
    results: &[veri_core::worker_pool::BatchResult],
    worker_count: usize,
    cache_dir: &Path,
) -> Result<()> {
    if results.is_empty() {
        return Ok(());
    }

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

/// Process results and print summary, returning (exit_code, failed_batches, total_tests_run, passed, failed, skipped, error)
fn process_and_summarize_results(
    results: &[veri_core::worker_pool::BatchResult],
    total_duration: Duration,
    worker_count: usize,
    verbose: bool,
) -> (i32, usize, usize, u32, u32, u32, u32) {
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

    (
        total_exit_code,
        failed_batches,
        total_tests_run,
        passed,
        failed,
        skipped,
        error,
    )
}

#[cfg(test)]
pub mod testing {
    use super::*;

    /// Mock implementation for testing
    pub struct MockExecutionService {
        pub result: ExecutionResult,
    }

    impl MockExecutionService {
        pub fn new(exit_code: ExitCode) -> Self {
            Self {
                result: ExecutionResult {
                    exit_code,
                    duration: Duration::from_secs(1),
                    total_tests_run: 10,
                    passed: 10,
                    failed: 0,
                    skipped: 0,
                    error: 0,
                },
            }
        }

        pub fn with_result(result: ExecutionResult) -> Self {
            Self { result }
        }
    }

    impl ExecutionService for MockExecutionService {
        fn execute_tests(
            &self,
            _nodeids: &[String],
            _tests_index: &TestsIndex,
            _run_options: &TestRunOptions,
        ) -> Result<ExecutionResult> {
            Ok(ExecutionResult {
                exit_code: self.result.exit_code,
                duration: self.result.duration,
                total_tests_run: self.result.total_tests_run,
                passed: self.result.passed,
                failed: self.result.failed,
                skipped: self.result.skipped,
                error: self.result.error,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    #[test]
    fn test_parse_worker_count_auto() {
        let wc = parse_worker_count(&Some("auto".to_string())).expect("parse auto");
        assert!(wc > 0, "auto should resolve to > 0 workers");
    }

    #[test]
    fn test_parse_worker_count_explicit() {
        let wc = parse_worker_count(&Some("4".to_string())).expect("parse 4");
        assert_eq!(wc, 4);
    }

    #[test]
    fn test_parse_worker_count_default() {
        let wc = parse_worker_count(&None).expect("parse None");
        assert!(wc > 0);
    }

    #[test]
    fn test_mock_execution_service() {
        let mock = MockExecutionService::new(ExitCode::Success);
        assert_eq!(mock.result.total_tests_run, 10);
    }
}
