//! Flaky test detection and handling

use anyhow::Result;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// Configuration for flaky test handling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlakyConfig {
    /// Number of retries for failed tests (default: 1)
    pub retry_count: u32,

    /// Maximum time to wait between retries (default: 1 second)
    pub retry_delay: Duration,

    /// Whether to enable automatic retry (default: true)
    pub auto_retry: bool,

    /// Threshold for marking a test as flaky (failures/total runs ratio)
    pub flaky_threshold: f64,

    /// Minimum number of runs before considering flaky threshold
    pub min_runs_for_flaky: u32,

    /// Whether to fail the overall run if flaky tests are detected
    pub fail_on_flaky: bool,

    /// Path to the flaky test database file
    pub flaky_db_path: PathBuf,
}

impl Default for FlakyConfig {
    fn default() -> Self {
        Self {
            retry_count: 1,
            retry_delay: Duration::from_secs(1),
            auto_retry: true,
            flaky_threshold: 0.2, // 20% failure rate
            min_runs_for_flaky: 5,
            fail_on_flaky: false,
            flaky_db_path: PathBuf::from(".veri/cache/flaky_tests.json"),
        }
    }
}

/// Database of flaky test history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlakyDatabase {
    /// Test run history
    pub test_history: HashMap<String, TestHistory>,

    /// Last updated timestamp
    pub last_updated: SystemTime,

    /// Version of the flaky database format
    pub version: String,
}

impl Default for FlakyDatabase {
    fn default() -> Self {
        Self {
            test_history: HashMap::new(),
            last_updated: SystemTime::now(),
            version: "1.0".to_string(),
        }
    }
}

/// History of a single test's runs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestHistory {
    /// Test node ID
    pub nodeid: String,

    /// Recent run results (most recent first)
    pub recent_runs: Vec<TestRun>,

    /// Total number of runs recorded
    pub total_runs: u32,

    /// Total number of failures
    pub total_failures: u32,

    /// Current flaky score (0.0 - 1.0)
    pub flaky_score: f64,

    /// Whether this test is currently marked as flaky
    pub is_flaky: bool,

    /// Last time this test was marked as flaky
    pub last_flaky_time: Option<SystemTime>,

    /// Number of consecutive passes (resets on failure)
    pub consecutive_passes: u32,
}

/// Record of a single test run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRun {
    /// Timestamp of the run
    pub timestamp: SystemTime,

    /// Whether the test passed
    pub passed: bool,

    /// Duration of the test run
    pub duration: Duration,

    /// Failure reason if the test failed
    pub failure_reason: Option<String>,

    /// Environment info (Python version, OS, etc.)
    pub environment: Option<String>,
}

/// Result of a test execution with retry handling
#[derive(Debug, Clone)]
pub struct TestExecutionResult {
    /// Final outcome (passed/failed)
    pub passed: bool,

    /// Number of attempts made
    pub attempts: u32,

    /// Results of each attempt
    pub attempt_results: Vec<TestRun>,

    /// Whether this test was retried
    pub was_retried: bool,

    /// Whether this test is marked as flaky
    pub is_flaky: bool,

    /// Final failure reason if failed
    pub failure_reason: Option<String>,
}

/// Flaky test manager
pub struct FlakyManager {
    config: FlakyConfig,
    database: FlakyDatabase,
    database_path: PathBuf,
    modified: bool,
}

impl FlakyManager {
    /// Create a new flaky manager with the given configuration
    pub fn new(config: FlakyConfig) -> Self {
        let database_path = config.flaky_db_path.clone();
        Self {
            config,
            database: FlakyDatabase::default(),
            database_path,
            modified: false,
        }
    }

    /// Load flaky database from disk
    pub fn load(&mut self) -> Result<()> {
        if self.database_path.exists() {
            let content = std::fs::read_to_string(&self.database_path)?;
            self.database = serde_json::from_str(&content)?;
            info!(
                "Loaded flaky database with {} test histories",
                self.database.test_history.len()
            );
        } else {
            info!("No existing flaky database found, starting fresh");
            self.database = FlakyDatabase::default();
        }

        // Clean up old entries
        self.cleanup_old_entries();

        Ok(())
    }

    /// Save flaky database to disk
    pub fn save(&mut self) -> Result<()> {
        if !self.modified {
            return Ok(());
        }

        // Ensure directory exists
        if let Some(parent) = self.database_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        self.database.last_updated = SystemTime::now();
        let content = serde_json::to_string_pretty(&self.database)?;
        std::fs::write(&self.database_path, content)?;

        info!("Saved flaky database to {}", self.database_path.display());
        self.modified = false;
        Ok(())
    }

    /// Execute a test with retry handling
    pub fn execute_test_with_retry<F>(
        &mut self,
        nodeid: &str,
        mut executor: F,
    ) -> Result<TestExecutionResult>
    where
        F: FnMut() -> Result<(bool, Duration, Option<String>)>,
    {
        let mut attempt_results = Vec::new();
        let mut final_passed = false;
        let mut final_failure_reason = None;

        let max_attempts = if self.config.auto_retry {
            self.config.retry_count + 1
        } else {
            1
        };

        for attempt in 1..=max_attempts {
            info!(
                "Executing test {} (attempt {}/{})",
                nodeid, attempt, max_attempts
            );

            let start_time = SystemTime::now();
            let (passed, duration, failure_reason) = executor()?;

            let test_run = TestRun {
                timestamp: start_time,
                passed,
                duration,
                failure_reason: failure_reason.clone(),
                environment: Some(self.get_environment_info()),
            };

            attempt_results.push(test_run);

            if passed {
                final_passed = true;
                break;
            } else {
                final_failure_reason = failure_reason;

                // Wait between retries (except for the last attempt)
                if attempt < max_attempts {
                    warn!(
                        "Test {} failed (attempt {}), retrying in {:?}...",
                        nodeid, attempt, self.config.retry_delay
                    );
                    std::thread::sleep(self.config.retry_delay);
                }
            }
        }

        let was_retried = attempt_results.len() > 1;

        // Update test history
        self.record_test_result(nodeid, &attempt_results)?;

        // Check if test is flaky
        let is_flaky = self.is_test_flaky(nodeid);

        Ok(TestExecutionResult {
            passed: final_passed,
            attempts: attempt_results.len() as u32,
            attempt_results,
            was_retried,
            is_flaky,
            failure_reason: final_failure_reason,
        })
    }

    /// Record test results in the history
    fn record_test_result(&mut self, nodeid: &str, runs: &[TestRun]) -> Result<()> {
        let mut total_runs = 0u32;
        let mut total_failures = 0u32;
        let mut consecutive_passes = 0u32;
        let mut recent_runs = Vec::new();

        // Get existing history if it exists
        if let Some(existing_history) = self.database.test_history.get(nodeid) {
            total_runs = existing_history.total_runs;
            total_failures = existing_history.total_failures;
            consecutive_passes = existing_history.consecutive_passes;
            recent_runs = existing_history.recent_runs.clone();
        }

        // Add new runs to history
        for run in runs {
            recent_runs.insert(0, run.clone());
            total_runs += 1;

            if !run.passed {
                total_failures += 1;
                consecutive_passes = 0;
            } else {
                consecutive_passes += 1;
            }
        }

        // Keep only recent runs (last 100)
        recent_runs.truncate(100);

        // Create or update history
        let mut history = TestHistory {
            nodeid: nodeid.to_string(),
            recent_runs,
            total_runs,
            total_failures,
            flaky_score: 0.0,
            is_flaky: false,
            last_flaky_time: None,
            consecutive_passes,
        };

        // Update flaky score
        self.update_flaky_score(&mut history);

        // Insert the updated history
        self.database
            .test_history
            .insert(nodeid.to_string(), history);

        self.modified = true;
        Ok(())
    }

    /// Update the flaky score for a test
    fn update_flaky_score(&mut self, history: &mut TestHistory) {
        if history.total_runs < self.config.min_runs_for_flaky {
            history.flaky_score = 0.0;
            history.is_flaky = false;
            return;
        }

        // Calculate failure rate
        let failure_rate = history.total_failures as f64 / history.total_runs as f64;

        // Update flaky score (weighted moving average)
        let recent_failure_rate = self.calculate_recent_failure_rate(&history.recent_runs);
        history.flaky_score = (failure_rate * 0.7) + (recent_failure_rate * 0.3);

        // Determine if test is flaky
        let was_flaky = history.is_flaky;
        history.is_flaky = history.flaky_score >= self.config.flaky_threshold;

        // If newly marked as flaky, record the time
        if history.is_flaky && !was_flaky {
            history.last_flaky_time = Some(SystemTime::now());
            warn!(
                "Test {} marked as flaky (score: {:.2})",
                history.nodeid, history.flaky_score
            );
        } else if !history.is_flaky && was_flaky && history.consecutive_passes >= 5 {
            // Test has been stable for a while, remove flaky status
            info!(
                "Test {} no longer considered flaky after {} consecutive passes",
                history.nodeid, history.consecutive_passes
            );
        }
    }

    /// Calculate failure rate from recent runs
    fn calculate_recent_failure_rate(&self, recent_runs: &[TestRun]) -> f64 {
        if recent_runs.is_empty() {
            return 0.0;
        }

        let recent_count = recent_runs.len().min(20); // Last 20 runs
        let recent_failures = recent_runs
            .iter()
            .take(recent_count)
            .filter(|run| !run.passed)
            .count();

        recent_failures as f64 / recent_count as f64
    }

    /// Check if a test is currently marked as flaky
    pub fn is_test_flaky(&self, nodeid: &str) -> bool {
        self.database
            .test_history
            .get(nodeid)
            .map(|history| history.is_flaky)
            .unwrap_or(false)
    }

    /// Get the flaky score for a test
    pub fn get_flaky_score(&self, nodeid: &str) -> f64 {
        self.database
            .test_history
            .get(nodeid)
            .map(|history| history.flaky_score)
            .unwrap_or(0.0)
    }

    /// Get all currently flaky tests
    pub fn get_flaky_tests(&self) -> Vec<String> {
        self.database
            .test_history
            .values()
            .filter(|history| history.is_flaky)
            .map(|history| history.nodeid.clone())
            .collect()
    }

    /// Generate a flaky test report
    pub fn generate_report(&self) -> FlakyReport {
        let flaky_tests: Vec<_> = self
            .database
            .test_history
            .values()
            .filter(|history| history.is_flaky)
            .cloned()
            .collect();

        let total_tests = self.database.test_history.len();
        let total_flaky = flaky_tests.len();

        let avg_flaky_score = if total_flaky > 0 {
            flaky_tests.iter().map(|h| h.flaky_score).sum::<f64>() / total_flaky as f64
        } else {
            0.0
        };

        let recommendations = self.generate_recommendations(&flaky_tests);

        FlakyReport {
            total_tests,
            total_flaky,
            flaky_percentage: if total_tests > 0 {
                (total_flaky as f64 / total_tests as f64) * 100.0
            } else {
                0.0
            },
            avg_flaky_score,
            flaky_tests,
            recommendations,
        }
    }

    /// Generate recommendations for dealing with flaky tests
    fn generate_recommendations(&self, flaky_tests: &[TestHistory]) -> Vec<String> {
        let mut recommendations = Vec::new();

        if flaky_tests.is_empty() {
            recommendations.push("✅ No flaky tests detected in your test suite!".to_string());
            return recommendations;
        }

        recommendations.push(format!(
            "🔍 Found {} flaky test(s) that may need attention",
            flaky_tests.len()
        ));

        // Analyze common patterns
        let timing_related = flaky_tests
            .iter()
            .filter(|h| {
                h.recent_runs.iter().any(|run| {
                    run.failure_reason
                        .as_ref()
                        .map(|reason| reason.contains("timeout") || reason.contains("timing"))
                        .unwrap_or(false)
                })
            })
            .count();

        if timing_related > 0 {
            recommendations.push(format!(
                "⏱️  {} test(s) appear to be timing-related - consider adding timeouts or sleeps",
                timing_related
            ));
        }

        let highly_flaky = flaky_tests.iter().filter(|h| h.flaky_score > 0.5).count();

        if highly_flaky > 0 {
            recommendations.push(format!(
                "🚨 {} test(s) are highly unstable (>50% failure rate) - prioritize fixing these",
                highly_flaky
            ));
        }

        recommendations.push("💡 Consider implementing test isolation, fixing race conditions, or mocking external dependencies".to_string());
        recommendations
            .push("📖 More info: https://docs.veri.dev/troubleshooting#flaky-tests".to_string());

        recommendations
    }

    /// Clean up old entries from the database
    fn cleanup_old_entries(&mut self) {
        let cutoff = SystemTime::now() - Duration::from_secs(30 * 24 * 60 * 60); // 30 days

        let mut to_remove = Vec::new();
        for (nodeid, history) in &self.database.test_history {
            if let Some(last_run) = history.recent_runs.first() {
                if last_run.timestamp < cutoff && !history.is_flaky {
                    to_remove.push(nodeid.clone());
                }
            }
        }

        for nodeid in to_remove {
            self.database.test_history.remove(&nodeid);
            self.modified = true;
        }
    }

    /// Get environment information for test runs
    fn get_environment_info(&self) -> String {
        format!(
            "{}-{}",
            std::env::consts::OS,
            std::env::var("PYTHON_VERSION").unwrap_or_else(|_| "unknown".to_string())
        )
    }
}

/// Report on flaky tests
#[derive(Debug, Clone)]
pub struct FlakyReport {
    pub total_tests: usize,
    pub total_flaky: usize,
    pub flaky_percentage: f64,
    pub avg_flaky_score: f64,
    pub flaky_tests: Vec<TestHistory>,
    pub recommendations: Vec<String>,
}

impl FlakyReport {
    /// Print a formatted flaky test report
    pub fn print_report(&self, use_color: bool) {
        let green = if use_color { "\x1b[32m" } else { "" };
        let yellow = if use_color { "\x1b[33m" } else { "" };
        let red = if use_color { "\x1b[31m" } else { "" };
        let reset = if use_color { "\x1b[0m" } else { "" };

        println!("🎯 Flaky Test Report");
        println!("====================");
        println!();

        println!("Summary:");
        println!("  Total tests tracked: {}", self.total_tests);

        if self.total_flaky == 0 {
            println!("  {}✅ No flaky tests detected{}", green, reset);
        } else {
            let color = if self.flaky_percentage > 10.0 {
                red
            } else {
                yellow
            };
            println!(
                "  {}⚠️  Flaky tests: {} ({:.1}%){}",
                color, self.total_flaky, self.flaky_percentage, reset
            );
            println!("  Average flaky score: {:.2}", self.avg_flaky_score);
        }

        if !self.flaky_tests.is_empty() {
            println!();
            println!("Flaky Tests:");

            let mut sorted_tests = self.flaky_tests.clone();
            sorted_tests.sort_by(|a, b| b.flaky_score.partial_cmp(&a.flaky_score).unwrap());

            for (i, test) in sorted_tests.iter().take(10).enumerate() {
                let color = if test.flaky_score > 0.5 { red } else { yellow };
                println!(
                    "  {}{}. {} (score: {:.2}, {}/{} failures){}",
                    color,
                    i + 1,
                    test.nodeid,
                    test.flaky_score,
                    test.total_failures,
                    test.total_runs,
                    reset
                );

                if let Some(last_failure) = test.recent_runs.iter().find(|run| !run.passed) {
                    if let Some(reason) = &last_failure.failure_reason {
                        println!(
                            "     Last failure: {}",
                            reason.lines().next().unwrap_or("Unknown")
                        );
                    }
                }
            }

            if sorted_tests.len() > 10 {
                println!("  ... and {} more flaky tests", sorted_tests.len() - 10);
            }
        }

        if !self.recommendations.is_empty() {
            println!();
            println!("Recommendations:");
            for recommendation in &self.recommendations {
                println!("  {}", recommendation);
            }
        }

        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_flaky_config_default() {
        let config = FlakyConfig::default();
        assert_eq!(config.retry_count, 1);
        assert!(config.auto_retry);
        assert_eq!(config.flaky_threshold, 0.2);
    }

    #[test]
    fn test_flaky_manager_creation() {
        let config = FlakyConfig::default();
        let manager = FlakyManager::new(config);
        assert_eq!(manager.database.test_history.len(), 0);
    }

    #[test]
    fn test_test_execution_with_retry() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let config = FlakyConfig {
            flaky_db_path: temp_dir.path().join("flaky.json"),
            ..Default::default()
        };
        let mut manager = FlakyManager::new(config);

        let mut attempt_count = 0;
        let result = manager.execute_test_with_retry("test_example", || {
            attempt_count += 1;
            if attempt_count == 1 {
                Ok((
                    false,
                    Duration::from_millis(100),
                    Some("Flaky failure".to_string()),
                ))
            } else {
                Ok((true, Duration::from_millis(50), None))
            }
        })?;

        assert!(result.passed);
        assert_eq!(result.attempts, 2);
        assert!(result.was_retried);

        Ok(())
    }

    #[test]
    fn test_flaky_score_calculation() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let config = FlakyConfig {
            min_runs_for_flaky: 2,
            flaky_threshold: 0.5,
            flaky_db_path: temp_dir.path().join("flaky.json"),
            ..Default::default()
        };
        let mut manager = FlakyManager::new(config);

        // Record multiple test runs
        let runs = vec![
            TestRun {
                timestamp: SystemTime::now(),
                passed: false,
                duration: Duration::from_millis(100),
                failure_reason: Some("Error 1".to_string()),
                environment: Some("test".to_string()),
            },
            TestRun {
                timestamp: SystemTime::now(),
                passed: true,
                duration: Duration::from_millis(50),
                failure_reason: None,
                environment: Some("test".to_string()),
            },
            TestRun {
                timestamp: SystemTime::now(),
                passed: false,
                duration: Duration::from_millis(120),
                failure_reason: Some("Error 2".to_string()),
                environment: Some("test".to_string()),
            },
        ];

        manager.record_test_result("test_flaky", &runs)?;

        let flaky_score = manager.get_flaky_score("test_flaky");
        assert!(flaky_score > 0.0);

        let is_flaky = manager.is_test_flaky("test_flaky");
        assert!(is_flaky); // 2/3 failures should exceed 0.5 threshold

        Ok(())
    }

    #[test]
    fn test_flaky_report_generation() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let config = FlakyConfig {
            flaky_db_path: temp_dir.path().join("flaky.json"),
            ..Default::default()
        };
        let manager = FlakyManager::new(config);

        let report = manager.generate_report();
        assert_eq!(report.total_tests, 0);
        assert_eq!(report.total_flaky, 0);
        assert!(!report.recommendations.is_empty());

        Ok(())
    }
}
