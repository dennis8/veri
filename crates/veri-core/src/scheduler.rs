//! Test scheduler for intelligent test ordering and worker assignment
//!
//! This module implements the scheduler logic for ordering tests and distributing
//! them across workers for optimal execution time and early failure detection.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::python_worker::{TestNode, TestsIndex};
use crate::schemas::TimingsData;

/// Test execution timing information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestTiming {
    pub nodeid: String,
    pub duration_ms: u64,
    pub last_outcome: TestOutcome,
    pub stability_score: f64, // 0.0 = flaky, 1.0 = stable
}

/// Test execution outcome
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TestOutcome {
    Passed,
    Failed,
    Skipped,
    Error,
}

/// Scheduled test batch for a worker
#[derive(Debug, Clone)]
pub struct TestBatch {
    pub worker_id: usize,
    pub nodeids: Vec<String>,
    pub estimated_duration_ms: u64,
    pub contains_last_failed: bool,
}

/// Test scheduling strategy
#[derive(Debug, Clone)]
pub enum SchedulingStrategy {
    /// Fastest execution: prioritize quick tests first
    Fastest,
    /// Fail-first: prioritize likely-to-fail tests first  
    FailFirst,
    /// Balanced: optimize for both speed and early failure detection
    Balanced,
}

/// Configuration for the scheduler
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    pub strategy: SchedulingStrategy,
    pub worker_count: usize,
    pub enable_long_pole_detection: bool,
    pub long_pole_threshold_ms: u64,
    pub enable_fail_first: bool,
    pub max_batch_duration_ms: u64,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            strategy: SchedulingStrategy::Balanced,
            worker_count: num_cpus::get().max(1),
            enable_long_pole_detection: true,
            long_pole_threshold_ms: 5000, // 5 seconds
            enable_fail_first: true,
            max_batch_duration_ms: 30000, // 30 seconds max per batch
        }
    }
}

/// Intelligent test scheduler
pub struct TestScheduler {
    config: SchedulerConfig,
    timings: HashMap<String, TestTiming>,
    last_failed: HashSet<String>,
}

impl TestScheduler {
    /// Create a new test scheduler
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            config,
            timings: HashMap::new(),
            last_failed: HashSet::new(),
        }
    }

    /// Load historical timing data
    pub fn load_timings(&mut self, timings_data: &TimingsData) -> Result<()> {
        self.timings.clear();

        for timing in &timings_data.timings() {
            let test_timing = TestTiming {
                nodeid: timing.nodeid.clone(),
                duration_ms: timing.duration_ms,
                last_outcome: match timing.outcome.as_str() {
                    "passed" => TestOutcome::Passed,
                    "failed" => TestOutcome::Failed,
                    "skipped" => TestOutcome::Skipped,
                    "error" => TestOutcome::Error,
                    _ => TestOutcome::Passed,
                },
                stability_score: timing.stability_score.unwrap_or(1.0),
            };

            self.timings
                .insert(timing.nodeid.clone(), test_timing.clone());

            // Track last failed tests
            if matches!(
                test_timing.last_outcome,
                TestOutcome::Failed | TestOutcome::Error
            ) {
                self.last_failed.insert(timing.nodeid.clone());
            }
        }

        Ok(())
    }

    /// Schedule tests across workers using intelligent ordering and bin-packing
    pub fn schedule_tests(
        &self,
        nodeids: &[String],
        tests_index: &TestsIndex,
    ) -> Result<Vec<TestBatch>> {
        if nodeids.is_empty() {
            return Ok(Vec::new());
        }

        // Step 1: Collect test information with timings
        let mut test_items = Vec::new();
        for nodeid in nodeids {
            let test_info = tests_index.tests.iter().find(|t| t.nodeid == *nodeid);

            let timing = self.timings.get(nodeid);
            let estimated_duration = timing
                .map(|t| t.duration_ms)
                .unwrap_or(self.estimate_test_duration(nodeid, test_info));

            let is_last_failed = self.last_failed.contains(nodeid);
            let stability_score = timing.map(|t| t.stability_score).unwrap_or(1.0);

            test_items.push(ScheduledTest {
                nodeid: nodeid.clone(),
                estimated_duration_ms: estimated_duration,
                is_last_failed,
                priority_score: self.calculate_priority_score(
                    estimated_duration,
                    is_last_failed,
                    stability_score,
                ),
            });
        }

        // Step 2: Sort tests by strategy
        self.sort_tests_by_strategy(&mut test_items[..]);

        // Step 3: Distribute across workers using bin-packing
        let batches = self.bin_pack_tests(test_items)?;

        Ok(batches)
    }

    /// Estimate duration for a test without historical data
    fn estimate_test_duration(&self, nodeid: &str, test_info: Option<&TestNode>) -> u64 {
        // Use heuristics based on test characteristics
        let base_duration = 100; // 100ms baseline

        let mut duration = base_duration;

        // Check test name patterns
        if nodeid.contains("integration") || nodeid.contains("e2e") {
            duration *= 10; // Integration tests are typically 10x slower
        } else if nodeid.contains("slow") || nodeid.contains("performance") {
            duration *= 5; // Performance tests are typically 5x slower
        } else if nodeid.contains("unit") || nodeid.contains("fast") {
            duration = base_duration; // Keep baseline for unit tests
        }

        // Check for parametrized tests (multiple runs)
        if nodeid.contains('[') && nodeid.contains(']') {
            duration = (duration as f64 * 1.5) as u64; // Parametrized tests often take longer
        }

        // Check test function patterns if we have test info
        if let Some(test) = test_info {
            if test.function.contains("test_") {
                // Standard test function (no change)
            } else if test.function.starts_with("test") {
                // Standard test naming (no change)
            }

            // Check for async markers
            if test.markers.iter().any(|m| m.contains("async")) {
                duration = (duration as f64 * 1.2) as u64;
            }

            // Check for fixture usage that might indicate setup overhead
            if test.fixtures.len() > 3 {
                duration = (duration as f64 * 1.3) as u64;
            }
        }

        duration
    }

    /// Calculate priority score for test ordering
    fn calculate_priority_score(
        &self,
        duration_ms: u64,
        is_last_failed: bool,
        stability_score: f64,
    ) -> f64 {
        let mut score = 0.0;

        match self.config.strategy {
            SchedulingStrategy::FailFirst => {
                // Prioritize failed tests and unstable tests
                if is_last_failed {
                    score += 1000.0;
                }
                score += (1.0 - stability_score) * 500.0; // Less stable = higher priority
                score -= duration_ms as f64 / 1000.0; // Shorter tests get slight boost
            }
            SchedulingStrategy::Fastest => {
                // Prioritize quick tests
                score += 1000.0 - (duration_ms as f64 / 100.0);
                if is_last_failed {
                    score += 100.0; // Small boost for failed tests
                }
            }
            SchedulingStrategy::Balanced => {
                // Balance between fail-first and speed
                if is_last_failed {
                    score += 500.0;
                }
                score += (1.0 - stability_score) * 250.0;
                score += 500.0 - (duration_ms as f64 / 200.0); // Moderate preference for speed
            }
        }

        score
    }

    /// Sort tests according to scheduling strategy
    fn sort_tests_by_strategy(&self, test_items: &mut [ScheduledTest]) {
        test_items.sort_by(|a, b| {
            // Primary sort: priority score (descending)
            b.priority_score
                .partial_cmp(&a.priority_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                // Secondary sort: nodeid for deterministic ordering
                .then_with(|| a.nodeid.cmp(&b.nodeid))
        });
    }

    /// Distribute tests across workers using bin-packing algorithm
    fn bin_pack_tests(&self, test_items: Vec<ScheduledTest>) -> Result<Vec<TestBatch>> {
        let mut workers: Vec<TestBatch> = Vec::new();

        // Initialize workers
        for i in 0..self.config.worker_count {
            workers.push(TestBatch {
                worker_id: i,
                nodeids: Vec::new(),
                estimated_duration_ms: 0,
                contains_last_failed: false,
            });
        }

        // Distribute tests using a best-fit approach
        for test in test_items {
            // Find the worker with the smallest current load that can fit this test
            let best_worker_idx = workers
                .iter()
                .enumerate()
                .filter(|(_, w)| {
                    // Check if adding this test would exceed max batch duration
                    w.estimated_duration_ms + test.estimated_duration_ms
                        <= self.config.max_batch_duration_ms
                })
                .min_by_key(|(_, w)| w.estimated_duration_ms)
                .map(|(idx, _)| idx)
                .unwrap_or_else(|| {
                    // If no worker can fit it within limits, use the least loaded one
                    workers
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, w)| w.estimated_duration_ms)
                        .map(|(idx, _)| idx)
                        .unwrap_or(0)
                });

            // Assign test to the selected worker
            let worker = &mut workers[best_worker_idx];
            worker.nodeids.push(test.nodeid);
            worker.estimated_duration_ms += test.estimated_duration_ms;
            if test.is_last_failed {
                worker.contains_last_failed = true;
            }
        }

        // Remove empty workers
        workers.retain(|w| !w.nodeids.is_empty());

        // Sort workers by priority (those with failed tests first)
        workers.sort_by(|a, b| {
            b.contains_last_failed
                .cmp(&a.contains_last_failed)
                .then_with(|| a.estimated_duration_ms.cmp(&b.estimated_duration_ms))
        });

        Ok(workers)
    }

    /// Get scheduling statistics for reporting
    pub fn get_scheduling_stats(&self, batches: &[TestBatch]) -> SchedulingStats {
        let total_tests: usize = batches.iter().map(|b| b.nodeids.len()).sum();
        let total_estimated_duration: u64 = batches.iter().map(|b| b.estimated_duration_ms).sum();

        let max_duration = batches
            .iter()
            .map(|b| b.estimated_duration_ms)
            .max()
            .unwrap_or(0);
        let min_duration = batches
            .iter()
            .map(|b| b.estimated_duration_ms)
            .min()
            .unwrap_or(0);

        let load_balance_ratio = if max_duration > 0 {
            min_duration as f64 / max_duration as f64
        } else {
            1.0
        };

        let failed_test_batches = batches.iter().filter(|b| b.contains_last_failed).count();

        SchedulingStats {
            total_tests,
            total_workers: batches.len(),
            total_estimated_duration_ms: total_estimated_duration,
            max_worker_duration_ms: max_duration,
            min_worker_duration_ms: min_duration,
            load_balance_ratio,
            batches_with_failed_tests: failed_test_batches,
        }
    }

    /// Format scheduling explanation for --explain mode
    pub fn format_explain(&self, batches: &[TestBatch], stats: &SchedulingStats) -> String {
        let mut output = Vec::new();

        output.push("Test Scheduling Plan:".to_string());
        output.push(format!("  Strategy: {:?}", self.config.strategy));
        output.push(format!("  Workers: {}", stats.total_workers));
        output.push(format!("  Total tests: {}", stats.total_tests));
        output.push(format!(
            "  Estimated duration: {:.1}s",
            stats.total_estimated_duration_ms as f64 / 1000.0
        ));
        output.push(format!(
            "  Load balance ratio: {:.1}%",
            stats.load_balance_ratio * 100.0
        ));

        if stats.batches_with_failed_tests > 0 {
            output.push(format!(
                "  ⚠️  {} workers have last-failed tests (prioritized)",
                stats.batches_with_failed_tests
            ));
        }

        output.push(String::new());
        output.push("Worker Distribution:".to_string());

        for batch in batches.iter() {
            let duration_sec = batch.estimated_duration_ms as f64 / 1000.0;
            let failed_indicator = if batch.contains_last_failed {
                " 🔥"
            } else {
                ""
            };

            output.push(format!(
                "  Worker {}: {} tests, {:.1}s estimated{}",
                batch.worker_id,
                batch.nodeids.len(),
                duration_sec,
                failed_indicator
            ));

            // Show first few tests for context
            if batch.nodeids.len() <= 3 {
                for nodeid in &batch.nodeids {
                    output.push(format!("    - {}", nodeid));
                }
            } else {
                for nodeid in batch.nodeids.iter().take(2) {
                    output.push(format!("    - {}", nodeid));
                }
                output.push(format!("    ... and {} more", batch.nodeids.len() - 2));
            }
        }

        output.join("\n")
    }
}

/// Internal structure for test scheduling
#[derive(Debug, Clone)]
struct ScheduledTest {
    nodeid: String,
    estimated_duration_ms: u64,
    is_last_failed: bool,
    priority_score: f64,
}

/// Scheduling statistics for reporting
#[derive(Debug)]
pub struct SchedulingStats {
    pub total_tests: usize,
    pub total_workers: usize,
    pub total_estimated_duration_ms: u64,
    pub max_worker_duration_ms: u64,
    pub min_worker_duration_ms: u64,
    pub load_balance_ratio: f64,
    pub batches_with_failed_tests: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_creation() {
        let config = SchedulerConfig::default();
        let scheduler = TestScheduler::new(config);
        assert_eq!(scheduler.timings.len(), 0);
        assert_eq!(scheduler.last_failed.len(), 0);
    }

    #[test]
    fn test_duration_estimation() {
        let config = SchedulerConfig::default();
        let scheduler = TestScheduler::new(config);

        // Integration test should be estimated higher
        let integration_duration = scheduler.estimate_test_duration("test_integration_flow", None);
        let unit_duration = scheduler.estimate_test_duration("test_unit_function", None);

        assert!(integration_duration > unit_duration);
    }

    #[test]
    fn test_priority_calculation_fail_first() {
        let config = SchedulerConfig {
            strategy: SchedulingStrategy::FailFirst,
            ..Default::default()
        };
        let scheduler = TestScheduler::new(config);

        let failed_score = scheduler.calculate_priority_score(1000, true, 0.8);
        let passed_score = scheduler.calculate_priority_score(1000, false, 0.8);

        assert!(failed_score > passed_score);
    }

    fn sample_generated_at() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339("2023-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    #[test]
    fn test_empty_schedule() {
        let config = SchedulerConfig::default();
        let scheduler = TestScheduler::new(config);
        let tests_index = TestsIndex {
            version: "1.0".to_string(),
            generated_at: sample_generated_at(),
            python_version: "3.12".to_string(),
            pytest_version: "8.0".to_string(),
            tests: Vec::new(),
            collection_errors: Vec::new(),
        };

        let batches = scheduler.schedule_tests(&[], &tests_index).unwrap();
        assert!(batches.is_empty());
    }

    #[test]
    fn test_single_test_scheduling() {
        let config = SchedulerConfig {
            worker_count: 2,
            ..Default::default()
        };
        let scheduler = TestScheduler::new(config);

        let tests_index = TestsIndex {
            version: "1.0.0".to_string(),
            generated_at: sample_generated_at(),
            python_version: "3.9.0".to_string(),
            pytest_version: "7.0.0".to_string(),
            tests: vec![TestNode {
                nodeid: "test_example.py::test_function".to_string(),
                path: "test_example.py".to_string(),
                line: 1,
                function: "test_function".to_string(),
                class: None,
                module: "test_example".to_string(),
                markers: vec![],
                fixtures: vec![],
                parametrize: None,
            }],
            collection_errors: Vec::new(),
        };

        let nodeids = vec!["test_example.py::test_function".to_string()];
        let batches = scheduler.schedule_tests(&nodeids, &tests_index).unwrap();

        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].nodeids.len(), 1);
        assert_eq!(batches[0].nodeids[0], "test_example.py::test_function");
    }
}
