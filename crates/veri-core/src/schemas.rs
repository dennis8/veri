#![allow(clippy::new_without_default)]
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const TESTS_INDEX_SCHEMA_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../schemas/tests.index.json"
));
pub const MARKERS_INDEX_SCHEMA_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../schemas/markers.index.json"
));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestsIndex {
    pub version: String,
    pub generated_at: DateTime<Utc>,
    pub python_version: String,
    pub pytest_version: String,
    pub tests: Vec<TestNode>,
    #[serde(default)]
    pub collection_errors: Vec<CollectionError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestNode {
    pub nodeid: String,
    pub path: String,
    pub line: u32,
    pub function: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    pub module: String,
    #[serde(default)]
    pub markers: Vec<String>,
    #[serde(default)]
    pub fixtures: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parametrize: Option<ParametrizeInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParametrizeInfo {
    pub params: Vec<String>,
    #[serde(default)]
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionError {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub error_type: String,
    pub message: String,
}

impl TestsIndex {
    pub fn new(python_version: String, pytest_version: String) -> Self {
        Self {
            version: "0.1.0".to_string(),
            generated_at: Utc::now(),
            python_version,
            pytest_version,
            tests: Vec::new(),
            collection_errors: Vec::new(),
        }
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleMap {
    pub version: String,
    pub generated_at: DateTime<Utc>,
    pub modules: HashMap<String, ModuleInfo>,
    pub packages: Vec<PackageInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInfo {
    pub module_name: String,
    pub is_package: bool,
    pub is_namespace: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_package: Option<String>,
    pub relative_path: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    pub name: String,
    pub path: String,
    pub is_namespace: bool,
    #[serde(default)]
    pub subpackages: Vec<String>,
}

impl ModuleMap {
    pub fn new() -> Self {
        Self {
            version: "0.1.0".to_string(),
            generated_at: Utc::now(),
            modules: HashMap::new(),
            packages: Vec::new(),
        }
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportsGraph {
    pub version: String,
    pub generated_at: DateTime<Utc>,
    pub edges: Vec<ImportEdge>,
    #[serde(default)]
    pub dynamic_imports: Vec<DynamicImport>,
    #[serde(default)]
    pub unresolved_imports: Vec<UnresolvedImport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportEdge {
    pub from_module: String,
    pub to_module: String,
    pub import_type: ImportType,
    pub line: u32,
    #[serde(default)]
    pub names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub is_conditional: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportType {
    Import,
    From,
    Relative,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicImport {
    pub from_module: String,
    pub line: u32,
    pub function: DynamicImportFunction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub argument: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DynamicImportFunction {
    #[serde(rename = "importlib.import_module")]
    ImportlibImportModule,
    #[serde(rename = "__import__")]
    BuiltinImport,
    #[serde(rename = "exec")]
    Exec,
    #[serde(rename = "eval")]
    Eval,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnresolvedImport {
    pub from_module: String,
    pub import_name: String,
    pub line: u32,
    pub is_third_party: bool,
    pub is_builtin: bool,
}

impl ImportsGraph {
    pub fn new() -> Self {
        Self {
            version: "0.1.0".to_string(),
            generated_at: Utc::now(),
            edges: Vec::new(),
            dynamic_imports: Vec::new(),
            unresolved_imports: Vec::new(),
        }
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tests_index_schema_json_is_valid() {
        serde_json::from_str::<serde_json::Value>(TESTS_INDEX_SCHEMA_JSON).unwrap();
    }

    #[test]
    fn markers_index_schema_json_is_valid() {
        serde_json::from_str::<serde_json::Value>(MARKERS_INDEX_SCHEMA_JSON).unwrap();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevdepsGraph {
    pub version: String,
    pub generated_at: DateTime<Utc>,
    pub reverse_deps: HashMap<String, ModuleReverseDeps>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleReverseDeps {
    #[serde(default)]
    pub direct_dependents: Vec<String>,
    #[serde(default)]
    pub transitive_dependents: Vec<String>,
    #[serde(default)]
    pub test_dependents: Vec<String>,
    #[serde(default)]
    pub uncertain_dependents: Vec<UncertainDependent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UncertainDependent {
    pub module: String,
    pub reason: String,
    pub confidence: f64,
}

impl RevdepsGraph {
    pub fn new() -> Self {
        Self {
            version: "0.1.0".to_string(),
            generated_at: Utc::now(),
            reverse_deps: HashMap::new(),
        }
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixturesMap {
    pub version: String,
    pub generated_at: DateTime<Utc>,
    pub fixtures: HashMap<String, FixtureInfo>,
    pub conftest_files: Vec<ConftestInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureInfo {
    pub name: String,
    pub scope: FixtureScope,
    pub defined_in: String,
    pub line: u32,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub autouse: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Vec<String>>,
    pub yield_fixture: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FixtureScope {
    Function,
    Class,
    Module,
    Package,
    Session,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConftestInfo {
    pub path: String,
    pub scope_path: String,
    #[serde(default)]
    pub fixtures: Vec<String>,
    #[serde(default)]
    pub plugins: Vec<String>,
    pub digest: String,
}

impl FixturesMap {
    pub fn new() -> Self {
        Self {
            version: "0.1.0".to_string(),
            generated_at: Utc::now(),
            fixtures: HashMap::new(),
            conftest_files: Vec::new(),
        }
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkersIndex {
    pub version: String,
    pub generated_at: DateTime<Utc>,
    pub markers: HashMap<String, MarkerInfo>,
    pub test_markers: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkerInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub registered: bool,
    pub usage_count: u32,
    pub first_seen: String,
    #[serde(default)]
    pub common_args: Vec<String>,
}

impl MarkersIndex {
    pub fn new() -> Self {
        Self {
            version: "0.1.0".to_string(),
            generated_at: Utc::now(),
            markers: HashMap::new(),
            test_markers: HashMap::new(),
        }
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestTimings {
    pub version: String,
    pub generated_at: DateTime<Utc>,
    pub runs: Vec<TimingRun>,
    pub aggregated_timings: HashMap<String, AggregatedTiming>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingRun {
    pub run_id: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub workers: u32,
    pub test_timings: HashMap<String, TestTiming>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestTiming {
    pub nodeid: String,
    pub setup_duration: f64,
    pub call_duration: f64,
    pub teardown_duration: f64,
    pub total_duration: f64,
    pub outcome: TestOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestOutcome {
    Passed,
    Failed,
    Skipped,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedTiming {
    pub nodeid: String,
    pub run_count: u32,
    pub avg_duration: f64,
    pub min_duration: f64,
    pub max_duration: f64,
    pub p50_duration: f64,
    pub p95_duration: f64,
    pub last_duration: f64,
    pub stability: f64,
}

impl TestTimings {
    pub fn new() -> Self {
        Self {
            version: "0.1.0".to_string(),
            generated_at: Utc::now(),
            runs: Vec::new(),
            aggregated_timings: HashMap::new(),
        }
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }

    /// Load from cache directory
    pub fn load_from_cache(cache_dir: &std::path::Path) -> Result<Self> {
        let timings_path = cache_dir.join("timings.json");
        if !timings_path.exists() {
            return Err(anyhow::anyhow!("Timings file not found"));
        }

        let content = std::fs::read_to_string(&timings_path)?;
        Self::from_json(&content)
    }
}

/// Type alias for compatibility with scheduler
pub type TimingsData = TestTimings;

/// Individual timing entry for compatibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingEntry {
    pub nodeid: String,
    pub duration_ms: u64,
    pub outcome: String,
    pub stability_score: Option<f64>,
}

impl TimingsData {
    /// Get timing entries in a format compatible with the scheduler
    pub fn timings(&self) -> Vec<TimingEntry> {
        let mut entries = Vec::new();

        // Convert from aggregated timings
        for (nodeid, timing) in &self.aggregated_timings {
            entries.push(TimingEntry {
                nodeid: nodeid.clone(),
                duration_ms: (timing.avg_duration * 1000.0) as u64,
                outcome: "passed".to_string(), // Default assumption
                stability_score: Some(timing.stability),
            });
        }

        // If no aggregated timings, try to use the most recent run
        if entries.is_empty() && !self.runs.is_empty() {
            if let Some(last_run) = self.runs.last() {
                for (nodeid, timing) in &last_run.test_timings {
                    entries.push(TimingEntry {
                        nodeid: nodeid.clone(),
                        duration_ms: (timing.total_duration * 1000.0) as u64,
                        outcome: match timing.outcome {
                            TestOutcome::Passed => "passed".to_string(),
                            TestOutcome::Failed => "failed".to_string(),
                            TestOutcome::Skipped => "skipped".to_string(),
                            TestOutcome::Error => "error".to_string(),
                        },
                        stability_score: None,
                    });
                }
            }
        }

        entries
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardsManifest {
    pub version: String,
    pub format_version: String,
    pub generated_at: DateTime<Utc>,
    pub total_shards: u32,
    pub strategy: ShardingStrategy,
    pub estimated_duration: f64,
    pub shards: Vec<Shard>,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShardingStrategy {
    RoundRobin,
    BinPack,
    TimingBased,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shard {
    pub shard_id: u32,
    pub estimated_duration: f64,
    pub test_count: u32,
    pub tests: Vec<ShardTest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardTest {
    pub nodeid: String,
    pub estimated_duration: f64,
    pub priority: u32,
    #[serde(default)]
    pub markers: Vec<String>,
}

impl ShardsManifest {
    pub fn new(total_shards: u32, strategy: ShardingStrategy) -> Self {
        Self {
            version: "0.1.0".to_string(),
            format_version: "veri-shards@1".to_string(),
            generated_at: Utc::now(),
            total_shards,
            strategy,
            estimated_duration: 0.0,
            shards: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum Event {
    #[serde(rename = "start")]
    Start {
        ts: DateTime<Utc>,
        run_id: String,
        veri_version: String,
        python_version: String,
        platform: String,
        workers: u32,
        cache_key: String,
    },
    #[serde(rename = "plan")]
    Plan {
        ts: DateTime<Utc>,
        run_id: String,
        total_tests: u32,
        selected_tests: u32,
        selection_reason: String,
        #[serde(default)]
        changed_files: Vec<String>,
        #[serde(default)]
        impacted_modules: Vec<String>,
    },
    #[serde(rename = "case")]
    Case {
        ts: DateTime<Utc>,
        run_id: String,
        nodeid: String,
        outcome: TestOutcome,
        duration: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        worker_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        longrepr: Option<String>,
        #[serde(default)]
        markers: Vec<String>,
    },
    #[serde(rename = "summary")]
    Summary {
        ts: DateTime<Utc>,
        run_id: String,
        total_duration: f64,
        passed: u32,
        failed: u32,
        skipped: u32,
        errors: u32,
        exit_code: u32,
    },
    #[serde(rename = "log")]
    Log {
        ts: DateTime<Utc>,
        run_id: String,
        level: LogLevel,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        nodeid: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogLevel {
    DEBUG,
    INFO,
    WARNING,
    ERROR,
}

impl Event {
    pub fn to_jsonl_line(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn from_jsonl_line(line: &str) -> Result<Self> {
        Ok(serde_json::from_str(line)?)
    }
}
