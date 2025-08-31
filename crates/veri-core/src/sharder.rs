use crate::python_worker::{PythonWorker, TestNode};
use crate::schemas::{Shard, ShardTest, ShardingStrategy, ShardsManifest, TimingsData};
use anyhow::Result;
use log::{debug, info, warn};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct SharderConfig {
    pub strategy: ShardingStrategy,
    pub min_shard_duration_ms: u64,
    pub max_shard_duration_ms: u64,
    pub load_balance_threshold: f64,
    pub enable_long_pole_first: bool,
}

impl Default for SharderConfig {
    fn default() -> Self {
        Self {
            strategy: ShardingStrategy::TimingBased,
            min_shard_duration_ms: 1000,   // 1 second minimum
            max_shard_duration_ms: 300000, // 5 minutes maximum
            load_balance_threshold: 0.9,   // 90% balance target
            enable_long_pole_first: true,
        }
    }
}

pub struct TestSharder {
    config: SharderConfig,
    work_dir: PathBuf,
    cache_dir: PathBuf,
}

impl TestSharder {
    pub fn new(work_dir: &Path, cache_dir: &Path) -> Self {
        Self {
            config: SharderConfig::default(),
            work_dir: work_dir.to_path_buf(),
            cache_dir: cache_dir.to_path_buf(),
        }
    }

    pub fn with_config(work_dir: &Path, cache_dir: &Path, config: SharderConfig) -> Self {
        Self {
            config,
            work_dir: work_dir.to_path_buf(),
            cache_dir: cache_dir.to_path_buf(),
        }
    }

    /// Split tests into N shards and generate manifest
    pub fn split_tests(
        &self,
        num_shards: u32,
        test_selection: Option<&[String]>,
    ) -> Result<ShardsManifest> {
        info!(
            "Splitting tests into {} shards using strategy: {:?}",
            num_shards, self.config.strategy
        );

        if num_shards == 0 {
            return Err(anyhow::anyhow!("Number of shards must be > 0"));
        }

        // Collect all tests
        let worker = PythonWorker::new(&self.work_dir, &self.cache_dir);
        let tests_index = worker.collect_tests(&[])?;

        // Filter tests if selection provided
        let filtered_tests = if let Some(selection) = test_selection {
            let selection_set: std::collections::HashSet<_> = selection.iter().collect();
            tests_index
                .tests
                .into_iter()
                .filter(|test| selection_set.contains(&test.nodeid))
                .collect()
        } else {
            tests_index.tests
        };

        if filtered_tests.is_empty() {
            warn!("No tests found to shard");
            return Ok(ShardsManifest::new(
                num_shards,
                self.config.strategy.clone(),
            ));
        }

        // Load timing data
        let timings = self.load_timing_data()?;

        // Apply sharding strategy
        let shards = match self.config.strategy {
            ShardingStrategy::RoundRobin => self.round_robin_sharding(&filtered_tests, num_shards),
            ShardingStrategy::BinPack => {
                self.bin_pack_sharding(&filtered_tests, num_shards, &timings)
            }
            ShardingStrategy::TimingBased => {
                self.timing_based_sharding(&filtered_tests, num_shards, &timings)
            }
        }?;

        // Calculate total estimated duration
        let total_duration: f64 = shards.iter().map(|s| s.estimated_duration).sum();

        // Create manifest
        let mut manifest = ShardsManifest::new(num_shards, self.config.strategy.clone());
        manifest.estimated_duration = total_duration;
        manifest.shards = shards;

        // Add metadata
        manifest.metadata.insert(
            "python_version".to_string(),
            serde_json::Value::String(
                std::env::var("PYTHON_VERSION").unwrap_or_else(|_| "unknown".to_string()),
            ),
        );
        manifest.metadata.insert(
            "platform".to_string(),
            serde_json::Value::String(std::env::consts::OS.to_string()),
        );

        info!(
            "Generated {} shards with total estimated duration: {:.1}s",
            manifest.shards.len(),
            total_duration
        );

        // Log balance statistics
        self.log_balance_stats(&manifest);

        Ok(manifest)
    }

    /// Get a specific shard from a manifest
    pub fn get_shard<'a>(
        &self,
        manifest: &'a ShardsManifest,
        shard_id: u32,
    ) -> Result<Option<&'a Shard>> {
        if shard_id >= manifest.total_shards {
            return Err(anyhow::anyhow!(
                "Shard ID {} is out of range (total shards: {})",
                shard_id,
                manifest.total_shards
            ));
        }

        Ok(manifest.shards.iter().find(|s| s.shard_id == shard_id))
    }

    /// Extract nodeids from a shard for execution
    pub fn extract_nodeids(&self, shard: &Shard) -> Vec<String> {
        shard.tests.iter().map(|t| t.nodeid.clone()).collect()
    }

    fn load_timing_data(&self) -> Result<HashMap<String, f64>> {
        match TimingsData::load_from_cache(&self.cache_dir) {
            Ok(timings_data) => {
                let mut timings = HashMap::new();
                for timing in timings_data.timings() {
                    timings.insert(timing.nodeid, timing.duration_ms as f64 / 1000.0);
                }
                info!("Loaded {} historical timings", timings.len());
                Ok(timings)
            }
            Err(_) => {
                warn!("No historical timing data found - using default estimates");
                Ok(HashMap::new())
            }
        }
    }

    fn round_robin_sharding(&self, tests: &[TestNode], num_shards: u32) -> Result<Vec<Shard>> {
        debug!(
            "Using round-robin sharding for {} tests across {} shards",
            tests.len(),
            num_shards
        );

        let mut shards: Vec<Shard> = (0..num_shards)
            .map(|i| Shard {
                shard_id: i,
                estimated_duration: 0.0,
                test_count: 0,
                tests: Vec::new(),
            })
            .collect();

        // Distribute tests in round-robin fashion
        for (i, test) in tests.iter().enumerate() {
            let shard_idx = i % num_shards as usize;
            let default_duration = 1.0; // 1 second default

            shards[shard_idx].tests.push(ShardTest {
                nodeid: test.nodeid.clone(),
                estimated_duration: default_duration,
                priority: 1, // Normal priority
                markers: test.markers.clone(),
            });
            shards[shard_idx].estimated_duration += default_duration;
            shards[shard_idx].test_count += 1;
        }

        Ok(shards)
    }

    fn bin_pack_sharding(
        &self,
        tests: &[TestNode],
        num_shards: u32,
        timings: &HashMap<String, f64>,
    ) -> Result<Vec<Shard>> {
        debug!(
            "Using bin-pack sharding for {} tests across {} shards",
            tests.len(),
            num_shards
        );

        // Create test items with estimated durations
        let mut test_items: Vec<(TestNode, f64)> = tests
            .iter()
            .map(|test| {
                let duration = timings.get(&test.nodeid).copied().unwrap_or(1.0); // Default 1 second
                (test.clone(), duration)
            })
            .collect();

        // Sort by duration descending (longest first for better bin packing)
        test_items.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        // Initialize shards
        let mut shards: Vec<Shard> = (0..num_shards)
            .map(|i| Shard {
                shard_id: i,
                estimated_duration: 0.0,
                test_count: 0,
                tests: Vec::new(),
            })
            .collect();

        // First-fit decreasing bin packing
        for (test, duration) in test_items {
            // Find shard with minimum current duration
            let min_shard_idx = shards
                .iter()
                .enumerate()
                .min_by(|a, b| {
                    a.1.estimated_duration
                        .partial_cmp(&b.1.estimated_duration)
                        .unwrap()
                })
                .map(|(i, _)| i)
                .unwrap();

            shards[min_shard_idx].tests.push(ShardTest {
                nodeid: test.nodeid.clone(),
                estimated_duration: duration,
                priority: 1,
                markers: test.markers.clone(),
            });
            shards[min_shard_idx].estimated_duration += duration;
            shards[min_shard_idx].test_count += 1;
        }

        Ok(shards)
    }

    fn timing_based_sharding(
        &self,
        tests: &[TestNode],
        num_shards: u32,
        timings: &HashMap<String, f64>,
    ) -> Result<Vec<Shard>> {
        debug!(
            "Using timing-based sharding for {} tests across {} shards",
            tests.len(),
            num_shards
        );

        // Create test items with priorities and durations
        let mut test_items: Vec<(TestNode, f64, u32)> = tests
            .iter()
            .map(|test| {
                let duration = timings.get(&test.nodeid).copied().unwrap_or(1.0);

                // Assign priorities: 0 = high (failed tests), 1 = normal
                let priority = if test.markers.contains(&"failed".to_string()) {
                    0
                } else {
                    1
                };

                (test.clone(), duration, priority)
            })
            .collect();

        // Sort by priority first (failed tests first), then by duration descending
        test_items.sort_by(|a, b| match a.2.cmp(&b.2) {
            std::cmp::Ordering::Equal => b.1.partial_cmp(&a.1).unwrap(),
            other => other,
        });

        // Initialize shards
        let mut shards: Vec<Shard> = (0..num_shards)
            .map(|i| Shard {
                shard_id: i,
                estimated_duration: 0.0,
                test_count: 0,
                tests: Vec::new(),
            })
            .collect();

        // Advanced bin packing with priority consideration
        for (test, duration, priority) in test_items {
            let target_shard_idx = if self.config.enable_long_pole_first && priority == 0 {
                // For high-priority (failed) tests, prefer shards that will start first
                // Distribute them across all shards to parallelize failures
                shards.iter().map(|s| s.tests.len()).sum::<usize>() % num_shards as usize
            } else {
                // For normal tests, use load balancing
                shards
                    .iter()
                    .enumerate()
                    .min_by(|a, b| {
                        a.1.estimated_duration
                            .partial_cmp(&b.1.estimated_duration)
                            .unwrap()
                    })
                    .map(|(i, _)| i)
                    .unwrap()
            };

            shards[target_shard_idx].tests.push(ShardTest {
                nodeid: test.nodeid.clone(),
                estimated_duration: duration,
                priority,
                markers: test.markers.clone(),
            });
            shards[target_shard_idx].estimated_duration += duration;
            shards[target_shard_idx].test_count += 1;
        }

        Ok(shards)
    }

    fn log_balance_stats(&self, manifest: &ShardsManifest) {
        if manifest.shards.is_empty() {
            return;
        }

        let durations: Vec<f64> = manifest
            .shards
            .iter()
            .map(|s| s.estimated_duration)
            .collect();

        let min_duration = durations.iter().copied().fold(f64::INFINITY, f64::min);
        let max_duration = durations.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let avg_duration = durations.iter().sum::<f64>() / durations.len() as f64;

        let balance_ratio = if max_duration > 0.0 {
            min_duration / max_duration
        } else {
            1.0
        };

        info!("Shard balance statistics:");
        info!("  Min duration: {:.1}s", min_duration);
        info!("  Max duration: {:.1}s", max_duration);
        info!("  Avg duration: {:.1}s", avg_duration);
        info!(
            "  Balance ratio: {:.1}% (target: {:.1}%)",
            balance_ratio * 100.0,
            self.config.load_balance_threshold * 100.0
        );

        if balance_ratio < self.config.load_balance_threshold {
            warn!("Load balance is below target threshold");
        }

        // Log per-shard details
        debug!("Per-shard breakdown:");
        for shard in &manifest.shards {
            debug!(
                "  Shard {}: {} tests, {:.1}s estimated",
                shard.shard_id, shard.test_count, shard.estimated_duration
            );
        }
    }

    /// Validate that a manifest is compatible with current test suite
    pub fn validate_manifest(&self, manifest: &ShardsManifest) -> Result<()> {
        // Check format version
        if manifest.format_version != "veri-shards@1" {
            return Err(anyhow::anyhow!(
                "Unsupported manifest format: {} (expected: veri-shards@1)",
                manifest.format_version
            ));
        }

        // Check shard IDs are sequential and complete
        let mut shard_ids: Vec<u32> = manifest.shards.iter().map(|s| s.shard_id).collect();
        shard_ids.sort();

        let expected_ids: Vec<u32> = (0..manifest.total_shards).collect();
        if shard_ids != expected_ids {
            return Err(anyhow::anyhow!(
                "Manifest has missing or duplicate shard IDs. Expected: {:?}, Found: {:?}",
                expected_ids,
                shard_ids
            ));
        }

        // Validate nodeids exist in current test suite
        let worker = PythonWorker::new(&self.work_dir, &self.cache_dir);
        if let Ok(tests_index) = worker.collect_tests(&[]) {
            let available_nodeids: std::collections::HashSet<_> =
                tests_index.tests.iter().map(|t| &t.nodeid).collect();

            for shard in &manifest.shards {
                for test in &shard.tests {
                    if !available_nodeids.contains(&test.nodeid) {
                        warn!(
                            "Manifest contains test not found in current suite: {}",
                            test.nodeid
                        );
                    }
                }
            }
        }

        info!("Manifest validation passed");
        Ok(())
    }

    /// Generate summary statistics for explain mode
    pub fn generate_stats_summary(&self, manifest: &ShardsManifest) -> ShardingStats {
        let total_tests: u32 = manifest.shards.iter().map(|s| s.test_count).sum();
        let durations: Vec<f64> = manifest
            .shards
            .iter()
            .map(|s| s.estimated_duration)
            .collect();

        let min_duration = durations.iter().copied().fold(f64::INFINITY, f64::min);
        let max_duration = durations.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let avg_duration = durations.iter().sum::<f64>() / durations.len() as f64;

        let balance_ratio = if max_duration > 0.0 {
            min_duration / max_duration
        } else {
            1.0
        };
        let efficiency = if manifest.estimated_duration > 0.0 {
            avg_duration / manifest.estimated_duration * manifest.shards.len() as f64
        } else {
            1.0
        };

        ShardingStats {
            total_shards: manifest.total_shards,
            total_tests,
            total_estimated_duration: manifest.estimated_duration,
            min_shard_duration: min_duration,
            max_shard_duration: max_duration,
            avg_shard_duration: avg_duration,
            balance_ratio,
            efficiency,
            strategy: manifest.strategy.clone(),
        }
    }

    /// Format explanation of sharding decisions
    pub fn format_explain(&self, manifest: &ShardsManifest, stats: &ShardingStats) -> String {
        let mut output = String::new();

        output.push_str("=== Sharding Analysis ===\n");
        output.push_str(&format!("Strategy: {:?}\n", stats.strategy));
        output.push_str(&format!("Total shards: {}\n", stats.total_shards));
        output.push_str(&format!("Total tests: {}\n", stats.total_tests));
        output.push_str(&format!(
            "Estimated duration: {:.1}s\n",
            stats.total_estimated_duration
        ));
        output.push('\n');

        output.push_str("Balance Analysis:\n");
        output.push_str(&format!("  Min shard: {:.1}s\n", stats.min_shard_duration));
        output.push_str(&format!("  Max shard: {:.1}s\n", stats.max_shard_duration));
        output.push_str(&format!("  Avg shard: {:.1}s\n", stats.avg_shard_duration));
        output.push_str(&format!("  Balance: {:.1}%\n", stats.balance_ratio * 100.0));
        output.push_str(&format!("  Efficiency: {:.1}%\n", stats.efficiency * 100.0));
        output.push('\n');

        output.push_str("Per-Shard Breakdown:\n");
        for shard in &manifest.shards {
            output.push_str(&format!(
                "  Shard {}: {} tests, {:.1}s\n",
                shard.shard_id, shard.test_count, shard.estimated_duration
            ));
        }

        output
    }
}

#[derive(Debug, Clone)]
pub struct ShardingStats {
    pub total_shards: u32,
    pub total_tests: u32,
    pub total_estimated_duration: f64,
    pub min_shard_duration: f64,
    pub max_shard_duration: f64,
    pub avg_shard_duration: f64,
    pub balance_ratio: f64,
    pub efficiency: f64,
    pub strategy: ShardingStrategy,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempdir::TempDir;

    #[test]
    fn test_round_robin_sharding() {
        let temp_dir = TempDir::new("veri_test").unwrap();
        let sharder = TestSharder::new(temp_dir.path(), temp_dir.path());

        let tests = vec![
            TestNode {
                nodeid: "test1".to_string(),
                path: "test1.py".to_string(),
                function: "test_a".to_string(),
                module: "test1".to_string(),
                markers: vec![],
                line: 1,
                class: None,
                fixtures: vec![],
                parametrize: None,
            },
            TestNode {
                nodeid: "test2".to_string(),
                path: "test2.py".to_string(),
                function: "test_b".to_string(),
                module: "test2".to_string(),
                markers: vec![],
                line: 1,
                class: None,
                fixtures: vec![],
                parametrize: None,
            },
            TestNode {
                nodeid: "test3".to_string(),
                path: "test3.py".to_string(),
                function: "test_c".to_string(),
                module: "test3".to_string(),
                markers: vec![],
                line: 1,
                class: None,
                fixtures: vec![],
                parametrize: None,
            },
        ];

        let shards = sharder.round_robin_sharding(&tests, 2).unwrap();

        assert_eq!(shards.len(), 2);
        assert_eq!(shards[0].test_count, 2); // test1, test3
        assert_eq!(shards[1].test_count, 1); // test2
    }

    #[test]
    fn test_bin_pack_sharding() {
        let temp_dir = TempDir::new("veri_test").unwrap();
        let sharder = TestSharder::new(temp_dir.path(), temp_dir.path());

        let tests = vec![
            TestNode {
                nodeid: "fast_test".to_string(),
                path: "test.py".to_string(),
                function: "test_fast".to_string(),
                module: "test".to_string(),
                markers: vec![],
                line: 1,
                class: None,
                fixtures: vec![],
                parametrize: None,
            },
            TestNode {
                nodeid: "slow_test".to_string(),
                path: "test.py".to_string(),
                function: "test_slow".to_string(),
                module: "test".to_string(),
                markers: vec![],
                line: 2,
                class: None,
                fixtures: vec![],
                parametrize: None,
            },
        ];

        let mut timings = HashMap::new();
        timings.insert("fast_test".to_string(), 1.0);
        timings.insert("slow_test".to_string(), 10.0);

        let shards = sharder.bin_pack_sharding(&tests, 2, &timings).unwrap();

        assert_eq!(shards.len(), 2);
        // Should distribute to balance loads
        let total_duration: f64 = shards.iter().map(|s| s.estimated_duration).sum();
        assert_eq!(total_duration, 11.0);
    }

    #[test]
    fn test_manifest_validation() {
        let temp_dir = TempDir::new("veri_test").unwrap();
        let sharder = TestSharder::new(temp_dir.path(), temp_dir.path());

        let mut manifest = ShardsManifest::new(2, ShardingStrategy::RoundRobin);
        manifest.shards = vec![
            Shard {
                shard_id: 0,
                estimated_duration: 5.0,
                test_count: 1,
                tests: vec![],
            },
            Shard {
                shard_id: 1,
                estimated_duration: 5.0,
                test_count: 1,
                tests: vec![],
            },
        ];

        // Should pass validation
        assert!(sharder.validate_manifest(&manifest).is_ok());

        // Test with missing shard
        manifest.shards.pop();
        assert!(sharder.validate_manifest(&manifest).is_err());
    }
}
