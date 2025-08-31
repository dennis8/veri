use log::warn;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::env;

/// Security configuration and plugin allowlist management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Whether to enforce plugin allowlist (default: true)
    pub enforce_allowlist: bool,

    /// List of allowed pytest plugins
    pub allowed_plugins: HashSet<String>,

    /// Whether to block network access (default: false for compatibility)
    pub no_network: bool,

    /// Whether to enable telemetry (default: false)
    pub telemetry_enabled: bool,

    /// Telemetry endpoint URL (only if enabled)
    pub telemetry_endpoint: Option<String>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            enforce_allowlist: true,
            allowed_plugins: Self::default_allowed_plugins(),
            no_network: false,
            telemetry_enabled: false,
            telemetry_endpoint: None,
        }
    }
}

impl SecurityConfig {
    /// Default set of safe, commonly used pytest plugins
    fn default_allowed_plugins() -> HashSet<String> {
        let mut plugins = HashSet::new();

        // Core testing plugins - generally safe
        plugins.insert("pytest".to_string());
        plugins.insert("pytest-cov".to_string());
        plugins.insert("pytest-mock".to_string());
        plugins.insert("pytest-xdist".to_string());
        plugins.insert("pytest-asyncio".to_string());
        plugins.insert("pytest-html".to_string());
        plugins.insert("pytest-json-report".to_string());
        plugins.insert("pytest-benchmark".to_string());
        plugins.insert("pytest-timeout".to_string());
        plugins.insert("pytest-randomly".to_string());
        plugins.insert("pytest-rerunfailures".to_string());
        plugins.insert("pytest-sugar".to_string());
        plugins.insert("pytest-clarity".to_string());

        // Framework-specific plugins
        plugins.insert("pytest-django".to_string());
        plugins.insert("pytest-flask".to_string());
        plugins.insert("pytest-tornado".to_string());
        plugins.insert("pytest-aiohttp".to_string());

        // Data/science plugins
        plugins.insert("pytest-datadir".to_string());
        plugins.insert("pytest-postgresql".to_string());
        plugins.insert("pytest-redis".to_string());
        plugins.insert("pytest-mysql".to_string());

        plugins
    }

    /// Load security configuration from environment and config
    pub fn from_config(_config: &crate::config::Config) -> Self {
        let mut security_config = Self::default();

        // Check environment variables for security overrides
        if env::var("VERI_NO_NETWORK").is_ok() {
            security_config.no_network = true;
        }

        if env::var("VERI_DISABLE_ALLOWLIST").is_ok() {
            warn!(
                "Plugin allowlist disabled via VERI_DISABLE_ALLOWLIST - this may reduce security"
            );
            security_config.enforce_allowlist = false;
        }

        if env::var("VERI_TELEMETRY_ENABLED").is_ok() {
            security_config.telemetry_enabled = true;
            if let Ok(endpoint) = env::var("VERI_TELEMETRY_ENDPOINT") {
                security_config.telemetry_endpoint = Some(endpoint);
            }
        }

        security_config
    }

    /// Check if a plugin is allowed to be loaded
    pub fn is_plugin_allowed(&self, plugin_name: &str) -> bool {
        if !self.enforce_allowlist {
            return true;
        }

        // Strip version info if present (e.g., "pytest-cov==5.0.0" -> "pytest-cov")
        // Support various version specifiers: ==, >=, <=, >, <, ~=, !=
        let clean_name = plugin_name
            .split(&['=', '>', '<', '~', '!'][..])
            .next()
            .unwrap_or(plugin_name);

        self.allowed_plugins.contains(clean_name)
    }

    /// Validate a list of plugins, returning allowed and blocked plugins
    pub fn validate_plugins(&self, plugins: &[String]) -> PluginValidationResult {
        let mut allowed = Vec::new();
        let mut blocked = Vec::new();

        for plugin in plugins {
            if self.is_plugin_allowed(plugin) {
                allowed.push(plugin.clone());
            } else {
                blocked.push(plugin.clone());
            }
        }

        PluginValidationResult { allowed, blocked }
    }

    /// Add a plugin to the allowlist
    pub fn allow_plugin(&mut self, plugin_name: String) {
        self.allowed_plugins.insert(plugin_name);
    }

    /// Remove a plugin from the allowlist
    pub fn block_plugin(&mut self, plugin_name: &str) {
        self.allowed_plugins.remove(plugin_name);
    }

    /// Check if network access should be blocked
    pub fn should_block_network(&self) -> bool {
        self.no_network
    }

    /// Check if telemetry is enabled
    pub fn is_telemetry_enabled(&self) -> bool {
        self.telemetry_enabled
    }
}

/// Result of plugin validation
#[derive(Debug, Clone)]
pub struct PluginValidationResult {
    pub allowed: Vec<String>,
    pub blocked: Vec<String>,
}

impl PluginValidationResult {
    /// Check if any plugins were blocked
    pub fn has_blocked_plugins(&self) -> bool {
        !self.blocked.is_empty()
    }

    /// Get a warning message for blocked plugins
    pub fn get_warning_message(&self) -> Option<String> {
        if self.blocked.is_empty() {
            return None;
        }

        Some(format!(
            "⚠️  Blocked plugins detected: {}\n\n\
            These plugins are not in the allowlist and may pose security risks.\n\
            To allow these plugins, add them to your veri.toml:\n\n\
            [security]\n\
            allowed_plugins = [{}]\n\n\
            Or disable allowlist enforcement (not recommended):\n\
            VERI_DISABLE_ALLOWLIST=1 veri\n\n\
            For more information: https://docs.veri.dev/security#plugin-allowlist",
            self.blocked.join(", "),
            self.blocked
                .iter()
                .map(|p| format!("\"{}\"", p))
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

/// Security scanner for detecting unsafe patterns
pub struct SecurityScanner;

impl SecurityScanner {
    /// Scan pytest plugins for known security issues
    pub fn scan_plugins(plugins: &[String]) -> Vec<SecurityWarning> {
        let mut warnings = Vec::new();

        for plugin in plugins {
            // Check for plugins known to have security implications
            if plugin.contains("exec") || plugin.contains("eval") {
                warnings.push(SecurityWarning {
                    severity: SecuritySeverity::High,
                    plugin_name: plugin.clone(),
                    issue: "Plugin name suggests code execution capabilities".to_string(),
                    recommendation: "Review plugin source code before allowing".to_string(),
                });
            }

            // Check for development/debugging plugins in production
            if plugin.contains("debug") || plugin.contains("pdb") {
                warnings.push(SecurityWarning {
                    severity: SecuritySeverity::Medium,
                    plugin_name: plugin.clone(),
                    issue: "Debug plugin detected - may expose sensitive information".to_string(),
                    recommendation: "Consider disabling in production environments".to_string(),
                });
            }

            // Check for network-related plugins
            if plugin.contains("http") || plugin.contains("requests") || plugin.contains("urllib") {
                warnings.push(SecurityWarning {
                    severity: SecuritySeverity::Low,
                    plugin_name: plugin.clone(),
                    issue: "Plugin may make network requests".to_string(),
                    recommendation: "Use --no-network flag if network isolation is required"
                        .to_string(),
                });
            }
        }

        warnings
    }

    /// Check for unsafe autoload patterns
    pub fn check_autoload_safety(conftest_files: &[String]) -> Vec<SecurityWarning> {
        let mut warnings = Vec::new();

        for conftest in conftest_files {
            // This would need to parse conftest.py files for unsafe patterns
            // For now, we'll implement basic checks
            if conftest.contains("exec(") || conftest.contains("eval(") {
                warnings.push(SecurityWarning {
                    severity: SecuritySeverity::High,
                    plugin_name: "conftest.py".to_string(),
                    issue: "Potentially unsafe code execution detected in conftest.py".to_string(),
                    recommendation: "Review conftest.py for security implications".to_string(),
                });
            }

            if conftest.contains("import subprocess") || conftest.contains("os.system") {
                warnings.push(SecurityWarning {
                    severity: SecuritySeverity::Medium,
                    plugin_name: "conftest.py".to_string(),
                    issue: "System command execution detected in conftest.py".to_string(),
                    recommendation: "Ensure subprocess calls are safe and necessary".to_string(),
                });
            }
        }

        warnings
    }
}

/// Security warning severity levels
#[derive(Debug, Clone, PartialEq)]
pub enum SecuritySeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl SecuritySeverity {
    pub fn emoji(&self) -> &'static str {
        match self {
            SecuritySeverity::Low => "ℹ️",
            SecuritySeverity::Medium => "⚠️",
            SecuritySeverity::High => "🚨",
            SecuritySeverity::Critical => "🔴",
        }
    }

    pub fn color_code(&self) -> &'static str {
        match self {
            SecuritySeverity::Low => "\x1b[36m",      // Cyan
            SecuritySeverity::Medium => "\x1b[33m",   // Yellow
            SecuritySeverity::High => "\x1b[31m",     // Red
            SecuritySeverity::Critical => "\x1b[91m", // Bright red
        }
    }
}

/// Security warning information
#[derive(Debug, Clone)]
pub struct SecurityWarning {
    pub severity: SecuritySeverity,
    pub plugin_name: String,
    pub issue: String,
    pub recommendation: String,
}

impl SecurityWarning {
    pub fn format(&self, use_color: bool) -> String {
        let color_start = if use_color {
            self.severity.color_code()
        } else {
            ""
        };
        let color_end = if use_color { "\x1b[0m" } else { "" };

        format!(
            "{} {}Security {}: {}{}\n  Plugin: {}\n  Issue: {}\n  Recommendation: {}",
            self.severity.emoji(),
            color_start,
            match self.severity {
                SecuritySeverity::Low => "Info",
                SecuritySeverity::Medium => "Warning",
                SecuritySeverity::High => "Alert",
                SecuritySeverity::Critical => "Critical",
            },
            self.issue,
            color_end,
            self.plugin_name,
            self.issue,
            self.recommendation
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_security_config() {
        let config = SecurityConfig::default();
        assert!(config.enforce_allowlist);
        assert!(!config.no_network);
        assert!(!config.telemetry_enabled);
        assert!(config.is_plugin_allowed("pytest-cov"));
        assert!(!config.is_plugin_allowed("unknown-plugin"));
    }

    #[test]
    fn test_plugin_validation() {
        let config = SecurityConfig::default();
        let plugins = vec![
            "pytest-cov".to_string(),
            "unknown-plugin".to_string(),
            "pytest-mock".to_string(),
        ];

        let result = config.validate_plugins(&plugins);
        assert_eq!(result.allowed.len(), 2);
        assert_eq!(result.blocked.len(), 1);
        assert!(result.has_blocked_plugins());
        assert!(result.get_warning_message().is_some());
    }

    #[test]
    fn test_plugin_version_stripping() {
        let config = SecurityConfig::default();
        assert!(config.is_plugin_allowed("pytest-cov==5.0.0"));
        assert!(config.is_plugin_allowed("pytest-mock>=3.0"));
        assert!(!config.is_plugin_allowed("unknown-plugin==1.0"));
    }

    #[test]
    fn test_security_scanner() {
        let plugins = vec![
            "pytest-exec-dangerous".to_string(),
            "pytest-debug".to_string(),
            "pytest-http-client".to_string(),
            "pytest-safe".to_string(),
        ];

        let warnings = SecurityScanner::scan_plugins(&plugins);
        assert!(warnings.len() >= 3); // Should warn about exec, debug, and http

        let high_severity_warnings: Vec<_> = warnings
            .iter()
            .filter(|w| w.severity == SecuritySeverity::High)
            .collect();
        assert!(!high_severity_warnings.is_empty());
    }

    #[test]
    fn test_env_var_overrides() {
        env::set_var("VERI_NO_NETWORK", "1");
        env::set_var("VERI_DISABLE_ALLOWLIST", "1");

        let dummy_config = crate::config::Config::default();
        let security_config = SecurityConfig::from_config(&dummy_config);

        assert!(security_config.no_network);
        assert!(!security_config.enforce_allowlist);

        // Clean up
        env::remove_var("VERI_NO_NETWORK");
        env::remove_var("VERI_DISABLE_ALLOWLIST");
    }
}
