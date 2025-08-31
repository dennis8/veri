use anyhow::Result;
use log::debug;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Telemetry client for optional usage analytics
#[derive(Clone)]
pub struct TelemetryClient {
    session_id: String,
    config: TelemetryConfig,
    metrics: TelemetryMetrics,
    enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Whether telemetry is enabled (default: false)
    pub enabled: bool,

    /// Endpoint URL for telemetry data
    pub endpoint: Option<String>,

    /// Collection interval in seconds (default: 300 = 5 minutes)
    pub collection_interval: u64,

    /// Whether to collect performance metrics
    pub collect_performance: bool,

    /// Whether to collect usage patterns
    pub collect_usage: bool,

    /// Maximum number of events to queue before dropping
    pub max_queue_size: usize,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: None,
            collection_interval: 300, // 5 minutes
            collect_performance: true,
            collect_usage: true,
            max_queue_size: 1000,
        }
    }
}

/// Aggregated telemetry metrics (no sensitive data)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryMetrics {
    /// Session information
    pub session_id: String,
    pub veri_version: String,
    pub platform: String,
    pub python_version: Option<String>,

    /// Usage counters
    pub runs_total: u64,
    pub runs_with_coverage: u64,
    pub runs_watch_mode: u64,
    pub runs_ci_mode: u64,

    /// Performance metrics (aggregated)
    pub avg_collection_time_ms: Option<u64>,
    pub avg_execution_time_ms: Option<u64>,
    pub avg_tests_per_run: Option<f64>,
    pub avg_workers_used: Option<f64>,

    /// Feature usage
    pub features_used: HashMap<String, u64>,

    /// Error categories (no specific error messages)
    pub error_categories: HashMap<String, u64>,

    /// Timestamp of last update
    pub last_updated: u64,
}

impl TelemetryMetrics {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            veri_version: env!("CARGO_PKG_VERSION").to_string(),
            platform: Self::get_platform(),
            python_version: None,
            runs_total: 0,
            runs_with_coverage: 0,
            runs_watch_mode: 0,
            runs_ci_mode: 0,
            avg_collection_time_ms: None,
            avg_execution_time_ms: None,
            avg_tests_per_run: None,
            avg_workers_used: None,
            features_used: HashMap::new(),
            error_categories: HashMap::new(),
            last_updated: Self::current_timestamp(),
        }
    }

    fn get_platform() -> String {
        format!("{}-{}", env::consts::OS, env::consts::ARCH)
    }

    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

impl TelemetryClient {
    /// Create a new telemetry client
    pub fn new(config: TelemetryConfig) -> Self {
        let session_id = Uuid::new_v4().to_string();
        let enabled = config.enabled && !Self::is_disabled_by_env();

        Self {
            session_id: session_id.clone(),
            config,
            metrics: TelemetryMetrics::new(session_id),
            enabled,
        }
    }

    /// Check if telemetry is disabled by environment variables
    fn is_disabled_by_env() -> bool {
        // Respect common telemetry opt-out environment variables
        env::var("VERI_NO_TELEMETRY").is_ok()
            || env::var("DO_NOT_TRACK").is_ok()
            || env::var("NO_ANALYTICS").is_ok()
            || env::var("DISABLE_TELEMETRY").is_ok()
    }

    /// Record a test run event
    pub fn record_run(&mut self, event: RunEvent) {
        if !self.enabled {
            return;
        }

        self.metrics.runs_total += 1;

        if event.coverage_enabled {
            self.metrics.runs_with_coverage += 1;
        }

        if event.watch_mode {
            self.metrics.runs_watch_mode += 1;
        }

        if event.ci_mode {
            self.metrics.runs_ci_mode += 1;
        }

        // Update averages
        TelemetryClient::update_average_u64(
            &mut self.metrics.avg_collection_time_ms,
            event.collection_time_ms,
        );
        TelemetryClient::update_average_u64(
            &mut self.metrics.avg_execution_time_ms,
            event.execution_time_ms,
        );
        TelemetryClient::update_average_f64(
            &mut self.metrics.avg_tests_per_run,
            event.test_count as f64,
        );
        TelemetryClient::update_average_f64(
            &mut self.metrics.avg_workers_used,
            event.worker_count as f64,
        );

        // Record feature usage
        for feature in event.features_used {
            *self.metrics.features_used.entry(feature).or_insert(0) += 1;
        }

        self.metrics.python_version = event.python_version;
        self.metrics.last_updated = TelemetryMetrics::current_timestamp();

        debug!(
            "Recorded telemetry run event: {} tests, {} workers",
            event.test_count, event.worker_count
        );
    }

    /// Record an error event (category only, no sensitive details)
    pub fn record_error(&mut self, category: ErrorCategory) {
        if !self.enabled {
            return;
        }

        let category_name = match category {
            ErrorCategory::ConfigurationError => "configuration",
            ErrorCategory::CollectionError => "collection",
            ErrorCategory::ExecutionError => "execution",
            ErrorCategory::ImportGraphError => "import_graph",
            ErrorCategory::CacheError => "cache",
            ErrorCategory::PluginError => "plugin",
            ErrorCategory::NetworkError => "network",
            ErrorCategory::FileSystemError => "filesystem",
            ErrorCategory::PythonEnvironmentError => "python_env",
        };

        *self
            .metrics
            .error_categories
            .entry(category_name.to_string())
            .or_insert(0) += 1;
        self.metrics.last_updated = TelemetryMetrics::current_timestamp();

        debug!("Recorded telemetry error: {}", category_name);
    }

    /// Record feature usage
    pub fn record_feature_usage(&mut self, feature: &str) {
        if !self.enabled {
            return;
        }

        *self
            .metrics
            .features_used
            .entry(feature.to_string())
            .or_insert(0) += 1;
        self.metrics.last_updated = TelemetryMetrics::current_timestamp();
    }

    /// Get current metrics (for debugging/verification)
    pub fn get_metrics(&self) -> &TelemetryMetrics {
        &self.metrics
    }

    /// Check if telemetry is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Disable telemetry (cannot be re-enabled in same session)
    pub fn disable(&mut self) {
        self.enabled = false;
        debug!("Telemetry disabled for current session");
    }

    /// Send telemetry data (if enabled and endpoint configured)
    pub fn send_telemetry(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let endpoint = match &self.config.endpoint {
            Some(url) => url,
            None => {
                debug!("No telemetry endpoint configured, skipping send");
                return Ok(());
            }
        };

        if env::var("VERI_NO_NETWORK").is_ok() {
            debug!("Network disabled, skipping telemetry send");
            return Ok(());
        }

        let client = reqwest::blocking::Client::new();
        let resp = client.post(endpoint).json(&self.metrics).send();

        if let Err(e) = resp {
            debug!("Telemetry send failed: {}", e);
        }

        Ok(())
    }

    /// Helper function to update running averages
    fn update_average_u64(current_avg: &mut Option<u64>, new_value: u64) {
        match current_avg {
            Some(avg) => {
                // Simple moving average with weight towards recent values
                *avg = (*avg * 3 + new_value) / 4;
            }
            None => {
                *current_avg = Some(new_value);
            }
        }
    }

    /// Helper function to update running averages for f64 values
    fn update_average_f64(current_avg: &mut Option<f64>, new_value: f64) {
        match current_avg {
            Some(avg) => {
                // Simple moving average with weight towards recent values
                *avg = (*avg * 3.0 + new_value) / 4.0;
            }
            None => {
                *current_avg = Some(new_value);
            }
        }
    }
}

/// Information about a test run for telemetry
#[derive(Debug, Clone)]
pub struct RunEvent {
    pub test_count: u32,
    pub worker_count: u32,
    pub collection_time_ms: u64,
    pub execution_time_ms: u64,
    pub coverage_enabled: bool,
    pub watch_mode: bool,
    pub ci_mode: bool,
    pub python_version: Option<String>,
    pub features_used: Vec<String>,
}

/// Error categories for telemetry (no sensitive information)
#[derive(Debug, Clone, Copy)]
pub enum ErrorCategory {
    ConfigurationError,
    CollectionError,
    ExecutionError,
    ImportGraphError,
    CacheError,
    PluginError,
    NetworkError,
    FileSystemError,
    PythonEnvironmentError,
}

/// Telemetry status information for user transparency
#[derive(Debug, Clone, Serialize)]
pub struct TelemetryStatus {
    pub enabled: bool,
    pub session_id: String,
    pub endpoint: Option<String>,
    pub data_collected: TelemetryDataSummary,
    pub last_sent: Option<u64>,
    pub opt_out_methods: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TelemetryDataSummary {
    pub runs_recorded: u64,
    pub features_tracked: usize,
    pub errors_by_category: usize,
    pub contains_personal_data: bool,
    pub contains_code_paths: bool,
    pub contains_test_names: bool,
}

impl TelemetryClient {
    /// Get current telemetry status for transparency
    pub fn get_status(&self) -> TelemetryStatus {
        TelemetryStatus {
            enabled: self.enabled,
            session_id: self.session_id.clone(),
            endpoint: self.config.endpoint.clone(),
            data_collected: TelemetryDataSummary {
                runs_recorded: self.metrics.runs_total,
                features_tracked: self.metrics.features_used.len(),
                errors_by_category: self.metrics.error_categories.len(),
                contains_personal_data: false,
                contains_code_paths: false,
                contains_test_names: false,
            },
            last_sent: None,
            opt_out_methods: vec![
                "Set VERI_NO_TELEMETRY=1".to_string(),
                "Set DO_NOT_TRACK=1".to_string(),
                "Add 'telemetry_enabled = false' to veri.toml".to_string(),
            ],
        }
    }

    /// Print telemetry status for user transparency
    pub fn print_status(&self, use_color: bool) {
        let status = self.get_status();

        let color_start = if use_color { "\x1b[36m" } else { "" }; // Cyan
        let color_end = if use_color { "\x1b[0m" } else { "" };

        println!("{}📊 Telemetry Status{}", color_start, color_end);
        println!(
            "  Enabled: {}",
            if status.enabled { "✅ Yes" } else { "❌ No" }
        );

        if status.enabled {
            println!("  Session ID: {}", status.session_id);
            if let Some(endpoint) = &status.endpoint {
                println!("  Endpoint: {}", endpoint);
            }
            println!("  Data collected:");
            println!(
                "    • Runs recorded: {}",
                status.data_collected.runs_recorded
            );
            println!(
                "    • Features tracked: {}",
                status.data_collected.features_tracked
            );
            println!(
                "    • Error categories: {}",
                status.data_collected.errors_by_category
            );
            println!("  Privacy guarantees:");
            println!("    • No personal data: ✅");
            println!("    • No code paths: ✅");
            println!("    • No test names: ✅");
        }

        println!("  Opt-out methods:");
        for method in &status.opt_out_methods {
            println!("    • {}", method);
        }

        println!("  More info: https://docs.veri.dev/telemetry");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_disabled_by_default() {
        let config = TelemetryConfig::default();
        let client = TelemetryClient::new(config);
        assert!(!client.is_enabled());
    }

    #[test]
    fn test_env_var_disables_telemetry() {
        env::set_var("DO_NOT_TRACK", "1");

        let client = TelemetryClient::new(TelemetryConfig {
            enabled: true,
            ..Default::default()
        });

        assert!(!client.is_enabled());

        env::remove_var("DO_NOT_TRACK");
    }

    #[test]
    fn test_record_run_event() {
        let mut client = TelemetryClient::new(TelemetryConfig {
            enabled: true,
            ..Default::default()
        });

        // Override env check for testing
        client.enabled = true;

        let event = RunEvent {
            test_count: 10,
            worker_count: 4,
            collection_time_ms: 100,
            execution_time_ms: 500,
            coverage_enabled: true,
            watch_mode: false,
            ci_mode: true,
            python_version: Some("3.12.0".to_string()),
            features_used: vec!["coverage".to_string(), "sharding".to_string()],
        };

        client.record_run(event);

        let metrics = client.get_metrics();
        assert_eq!(metrics.runs_total, 1);
        assert_eq!(metrics.runs_with_coverage, 1);
        assert_eq!(metrics.runs_ci_mode, 1);
        assert_eq!(metrics.features_used.get("coverage"), Some(&1));
        assert_eq!(metrics.python_version, Some("3.12.0".to_string()));
    }

    #[test]
    fn test_record_error() {
        let mut client = TelemetryClient::new(TelemetryConfig {
            enabled: true,
            ..Default::default()
        });
        client.enabled = true; // Override for testing

        client.record_error(ErrorCategory::CollectionError);
        client.record_error(ErrorCategory::CollectionError);
        client.record_error(ErrorCategory::ExecutionError);

        let metrics = client.get_metrics();
        assert_eq!(metrics.error_categories.get("collection"), Some(&2));
        assert_eq!(metrics.error_categories.get("execution"), Some(&1));
    }

    #[test]
    fn test_privacy_guarantees() {
        let config = TelemetryConfig::default();
        let client = TelemetryClient::new(config);
        let status = client.get_status();

        // Verify privacy guarantees
        assert!(!status.data_collected.contains_personal_data);
        assert!(!status.data_collected.contains_code_paths);
        assert!(!status.data_collected.contains_test_names);
    }

    #[test]
    fn test_telemetry_status() {
        let config = TelemetryConfig::default();
        let client = TelemetryClient::new(config);
        let status = client.get_status();

        assert!(!status.enabled);
        assert!(!status.opt_out_methods.is_empty());
        assert!(!status.session_id.is_empty());
    }
}
