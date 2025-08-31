use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    /// Number of parallel workers
    pub workers: Option<String>,
    
    /// Cache directory path
    pub cache_dir: Option<PathBuf>,
    
    /// Default log level
    pub log_level: Option<String>,
    
    /// Disable colored output
    pub no_color: Option<bool>,
    
    /// Test discovery patterns
    pub test_paths: Option<Vec<String>>,
    
    /// Default markers to run
    pub markers: Option<String>,
    
    /// Default keyword expression
    pub keyword: Option<String>,
    
    /// JUnit XML output path
    pub junit_xml: Option<PathBuf>,
    
    /// JSONL output path
    pub jsonl: Option<PathBuf>,
    
    /// Enable coverage by default
    pub cov: Option<bool>,
    
    /// Coverage merge mode
    pub cov_merge_full: Option<bool>,
    
    /// Maximum failures before stopping
    pub maxfail: Option<u32>,
    
    /// Verbosity level
    pub verbose: Option<u8>,
    
    /// Quiet mode
    pub quiet: Option<bool>,
    
    /// Last failed mode
    pub last_failed: Option<bool>,
    
    /// Default engine
    pub engine: Option<String>,
    
    /// Watch mode
    pub watch: Option<bool>,
    
    /// Run all tests by default
    pub all: Option<bool>,
    
    /// Security configuration
    pub security: Option<SecurityConfig>,
    
    /// Telemetry configuration
    pub telemetry: Option<TelemetryConfig>,
    
    /// Flaky test configuration
    pub flaky: Option<FlakyTestConfig>,
    
    /// Automatic retry of failed tests
    pub auto_retry: Option<bool>,
    
    /// Number of retries for failed tests
    pub retry_count: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SecurityConfig {
    /// Whether to enforce plugin allowlist (default: true)
    pub enforce_allowlist: Option<bool>,
    
    /// List of allowed pytest plugins
    pub allowed_plugins: Option<Vec<String>>,
    
    /// Whether to block network access (default: false)
    pub no_network: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelemetryConfig {
    /// Whether telemetry is enabled (default: false)
    pub enabled: Option<bool>,
    
    /// Endpoint URL for telemetry data
    pub endpoint: Option<String>,
    
    /// Collection interval in seconds
    pub collection_interval: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FlakyTestConfig {
    /// Number of retries for failed tests (default: 1)
    pub retry_count: Option<u32>,
    
    /// Whether to enable automatic retry (default: true)
    pub auto_retry: Option<bool>,
    
    /// Threshold for marking a test as flaky (default: 0.2)
    pub flaky_threshold: Option<f64>,
    
    /// Minimum number of runs before considering flaky threshold (default: 5)
    pub min_runs_for_flaky: Option<u32>,
    
    /// Whether to fail the overall run if flaky tests are detected (default: false)
    pub fail_on_flaky: Option<bool>,
}

impl Default for FlakyTestConfig {
    fn default() -> Self {
        Self {
            retry_count: Some(1),
            auto_retry: Some(true),
            flaky_threshold: Some(0.2),
            min_runs_for_flaky: Some(5),
            fail_on_flaky: Some(false),
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            enforce_allowlist: Some(true),
            allowed_plugins: None,
            no_network: Some(false),
        }
    }
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: Some(false),
            endpoint: None,
            collection_interval: Some(300),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            workers: None,
            cache_dir: Some(PathBuf::from(".veri/cache")),
            log_level: Some("INFO".to_string()),
            no_color: Some(false),
            test_paths: None,
            markers: None,
            keyword: None,
            junit_xml: None,
            jsonl: None,
            cov: Some(false),
            cov_merge_full: Some(false),
            maxfail: None,
            verbose: Some(0),
            quiet: Some(false),
            last_failed: Some(false),
            engine: Some("veri".to_string()),
            watch: Some(false),
            all: Some(false),
            security: None,
            telemetry: None,
            flaky: None,
            auto_retry: Some(false),
            retry_count: Some(1),
        }
    }
}

impl Config {
    /// Load configuration with precedence: CLI flags > config file > environment > defaults
    pub fn load(config_path: Option<&Path>) -> Result<Self> {
        let mut config = Self::default();
        
        // First, apply environment variables
        config.apply_env_vars();
        
        // Then, try to load from config file
        if let Some(file_config) = Self::load_from_file(config_path)? {
            config.merge(file_config);
        }
        
        Ok(config)
    }
    
    /// Load configuration from file
    pub fn load_from_file(config_path: Option<&Path>) -> Result<Option<Self>> {
        // If explicit config path is provided, use it
        if let Some(path) = config_path {
            if path.exists() {
                let content = std::fs::read_to_string(path)
                    .context(format!("Failed to read config file: {}", path.display()))?;
                let config: Self = toml::from_str(&content)
                    .context(format!("Failed to parse config file: {}", path.display()))?;
                return Ok(Some(config));
            } else {
                anyhow::bail!("Config file not found: {}", path.display());
            }
        }
        
        // Look for veri.toml in current directory and parents
        let mut current_dir = env::current_dir()?;
        loop {
            let veri_toml = current_dir.join("veri.toml");
            if veri_toml.exists() {
                let content = std::fs::read_to_string(&veri_toml)?;
                let config: Self = toml::from_str(&content)?;
                return Ok(Some(config));
            }
            
            // Also check for [tool.veri] in pyproject.toml
            let pyproject_toml = current_dir.join("pyproject.toml");
            if pyproject_toml.exists() {
                let content = std::fs::read_to_string(&pyproject_toml)?;
                if let Ok(parsed) = toml::from_str::<toml::Value>(&content) {
                    if let Some(tool) = parsed.get("tool") {
                        if let Some(veri_config) = tool.get("veri") {
                            if let Ok(config) = veri_config.clone().try_into::<Self>() {
                                return Ok(Some(config));
                            }
                        }
                    }
                }
            }
            
            // Move to parent directory
            if let Some(parent) = current_dir.parent() {
                current_dir = parent.to_path_buf();
            } else {
                break;
            }
        }
        
        Ok(None)
    }
    
    /// Apply environment variables
    fn apply_env_vars(&mut self) {
        if let Ok(log_level) = env::var("VERI_LOG") {
            self.log_level = Some(log_level);
        }
        
        if let Ok(_) = env::var("VERI_NO_COLOR") {
            self.no_color = Some(true);
        }
        
        if let Ok(cache_dir) = env::var("VERI_CACHE_DIR") {
            self.cache_dir = Some(PathBuf::from(cache_dir));
        }
        
        if let Ok(workers) = env::var("VERI_WORKERS") {
            self.workers = Some(workers);
        }
        
        // Security-related environment variables
        if let Ok(_) = env::var("VERI_NO_NETWORK") {
            if self.security.is_none() {
                self.security = Some(SecurityConfig::default());
            }
            if let Some(ref mut security) = self.security {
                security.no_network = Some(true);
            }
        }
        
        if let Ok(_) = env::var("VERI_DISABLE_ALLOWLIST") {
            if self.security.is_none() {
                self.security = Some(SecurityConfig::default());
            }
            if let Some(ref mut security) = self.security {
                security.enforce_allowlist = Some(false);
            }
        }
        
        // Telemetry environment variables
        if let Ok(_) = env::var("VERI_TELEMETRY_ENABLED") {
            if self.telemetry.is_none() {
                self.telemetry = Some(TelemetryConfig::default());
            }
            if let Some(ref mut telemetry) = self.telemetry {
                telemetry.enabled = Some(true);
            }
        }
        
        if let Ok(endpoint) = env::var("VERI_TELEMETRY_ENDPOINT") {
            if self.telemetry.is_none() {
                self.telemetry = Some(TelemetryConfig::default());
            }
            if let Some(ref mut telemetry) = self.telemetry {
                telemetry.endpoint = Some(endpoint);
            }
        }
    }
    
    /// Merge another config into this one, keeping non-None values from other
    fn merge(&mut self, other: Self) {
        if other.workers.is_some() { self.workers = other.workers; }
        if other.cache_dir.is_some() { self.cache_dir = other.cache_dir; }
        if other.log_level.is_some() { self.log_level = other.log_level; }
        if other.no_color.is_some() { self.no_color = other.no_color; }
        if other.test_paths.is_some() { self.test_paths = other.test_paths; }
        if other.markers.is_some() { self.markers = other.markers; }
        if other.keyword.is_some() { self.keyword = other.keyword; }
        if other.junit_xml.is_some() { self.junit_xml = other.junit_xml; }
        if other.jsonl.is_some() { self.jsonl = other.jsonl; }
        if other.cov.is_some() { self.cov = other.cov; }
        if other.cov_merge_full.is_some() { self.cov_merge_full = other.cov_merge_full; }
        if other.maxfail.is_some() { self.maxfail = other.maxfail; }
        if other.verbose.is_some() { self.verbose = other.verbose; }
        if other.quiet.is_some() { self.quiet = other.quiet; }
        if other.last_failed.is_some() { self.last_failed = other.last_failed; }
        if other.engine.is_some() { self.engine = other.engine; }
        if other.watch.is_some() { self.watch = other.watch; }
        if other.all.is_some() { self.all = other.all; }
        if other.security.is_some() { self.security = other.security; }
        if other.telemetry.is_some() { self.telemetry = other.telemetry; }
    }
    
    /// Apply CLI arguments to override config values
    pub fn apply_cli_args(&mut self, all: bool, watch: bool, keyword: Option<String>, marker: Option<String>, 
                         workers: Option<String>, last_failed: bool, junit_xml: Option<std::path::PathBuf>, 
                         jsonl: Option<std::path::PathBuf>, maxfail: Option<u32>, verbose: u8, quiet: bool, 
                         cov: bool, cov_merge_full: bool, no_capture: bool, engine: String,
                         no_network: bool, disable_allowlist: bool) {
        if workers.is_some() { self.workers = workers; }
        if keyword.is_some() { self.keyword = keyword; }
        if marker.is_some() { self.markers = marker; }
        if junit_xml.is_some() { self.junit_xml = junit_xml; }
        if jsonl.is_some() { self.jsonl = jsonl; }
        if maxfail.is_some() { self.maxfail = maxfail; }
        if verbose > 0 { self.verbose = Some(verbose); }
        if quiet { self.quiet = Some(true); }
        if last_failed { self.last_failed = Some(true); }
        if watch { self.watch = Some(true); }
        if all { self.all = Some(true); }
        if cov { self.cov = Some(true); }
        if cov_merge_full { self.cov_merge_full = Some(true); }
        if no_capture { /* handle capture mode */ }
        
        // Handle security flags
        if no_network || disable_allowlist {
            if self.security.is_none() {
                self.security = Some(SecurityConfig::default());
            }
            if let Some(ref mut security) = self.security {
                if no_network {
                    security.no_network = Some(true);
                }
                if disable_allowlist {
                    security.enforce_allowlist = Some(false);
                }
            }
        }
        
        // Engine conversion
        self.engine = Some(engine);
    }
    
    /// Get effective cache directory
    pub fn cache_dir(&self) -> PathBuf {
        self.cache_dir.clone().unwrap_or_else(|| PathBuf::from(".veri/cache"))
    }
    
    /// Get effective log level
    pub fn log_level(&self) -> &str {
        self.log_level.as_deref().unwrap_or("INFO")
    }
    
    /// Check if colored output should be disabled
    pub fn no_color(&self) -> bool {
        self.no_color.unwrap_or(false) || env::var("NO_COLOR").is_ok()
    }
    
    /// Get security configuration with defaults
    pub fn security(&self) -> SecurityConfig {
        self.security.clone().unwrap_or_default()
    }
    
    /// Get telemetry configuration with defaults
    pub fn telemetry(&self) -> TelemetryConfig {
        self.telemetry.clone().unwrap_or_default()
    }
    
    /// Check if network access should be blocked
    pub fn should_block_network(&self) -> bool {
        self.security().no_network.unwrap_or(false) || env::var("VERI_NO_NETWORK").is_ok()
    }
    
    /// Check if plugin allowlist should be enforced
    pub fn enforce_plugin_allowlist(&self) -> bool {
        if env::var("VERI_DISABLE_ALLOWLIST").is_ok() {
            return false;
        }
        self.security().enforce_allowlist.unwrap_or(true)
    }
    
    /// Check if telemetry is enabled
    pub fn is_telemetry_enabled(&self) -> bool {
        // Respect opt-out environment variables
        if env::var("VERI_NO_TELEMETRY").is_ok() || 
           env::var("DO_NOT_TRACK").is_ok() || 
           env::var("NO_ANALYTICS").is_ok() {
            return false;
        }
        
        self.telemetry().enabled.unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::NamedTempFile;
    use std::io::Write;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.cache_dir(), PathBuf::from(".veri/cache"));
        assert_eq!(config.log_level(), "INFO");
        assert!(!config.no_color());
    }

    #[test]
    fn test_env_vars() {
        env::set_var("VERI_LOG", "DEBUG");
        env::set_var("VERI_NO_COLOR", "1");
        env::set_var("VERI_CACHE_DIR", "/tmp/veri");
        
        let mut config = Config::default();
        config.apply_env_vars();
        
        assert_eq!(config.log_level(), "DEBUG");
        assert!(config.no_color());
        assert_eq!(config.cache_dir(), PathBuf::from("/tmp/veri"));
        
        // Clean up
        env::remove_var("VERI_LOG");
        env::remove_var("VERI_NO_COLOR");
        env::remove_var("VERI_CACHE_DIR");
    }

    #[test]
    fn test_load_config_file() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, r#"
workers = "4"
log_level = "DEBUG"
cache_dir = "/custom/cache"
cov = true
        "#).unwrap();
        
        let config = Config::load_from_file(Some(file.path())).unwrap().unwrap();
        assert_eq!(config.workers, Some("4".to_string()));
        assert_eq!(config.log_level, Some("DEBUG".to_string()));
        assert_eq!(config.cache_dir, Some(PathBuf::from("/custom/cache")));
        assert_eq!(config.cov, Some(true));
    }
}