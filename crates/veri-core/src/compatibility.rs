//! Plugin compatibility detection and matrix testing support

use crate::python_worker::PythonWorker;
use anyhow::Result;
use log::info;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Compatibility matrix for testing veri across different environments
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatibilityMatrix {
    /// Python versions to test (e.g., "3.9", "3.10", "3.11", "3.12", "3.13")
    pub python_versions: Vec<String>,

    /// Operating systems to test ("linux", "macos", "windows")
    pub operating_systems: Vec<String>,

    /// Environment types ("venv", "conda", "poetry", "uv", "system")
    pub environment_types: Vec<String>,

    /// Package managers ("pip", "conda", "uv", "poetry")
    pub package_managers: Vec<String>,

    /// Known plugin compatibility issues
    pub plugin_compatibility: HashMap<String, PluginCompatibilityInfo>,
}

impl Default for CompatibilityMatrix {
    fn default() -> Self {
        Self {
            python_versions: vec![
                "3.9".to_string(),
                "3.10".to_string(),
                "3.11".to_string(),
                "3.12".to_string(),
                "3.13".to_string(),
            ],
            operating_systems: vec![
                "linux".to_string(),
                "macos".to_string(),
                "windows".to_string(),
            ],
            environment_types: vec![
                "venv".to_string(),
                "conda".to_string(),
                "poetry".to_string(),
                "uv".to_string(),
                "system".to_string(),
            ],
            package_managers: vec![
                "pip".to_string(),
                "conda".to_string(),
                "uv".to_string(),
                "poetry".to_string(),
            ],
            plugin_compatibility: Self::default_plugin_compatibility(),
        }
    }
}

impl CompatibilityMatrix {
    /// Load compatibility matrix from file or use default
    pub fn load_or_default<P: AsRef<Path>>(path: P) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                let matrix: Self = toml::from_str(&content)?;
                Ok(matrix)
            }
            Err(_) => {
                info!("No compatibility matrix found, using defaults");
                Ok(Self::default())
            }
        }
    }

    /// Default plugin compatibility information
    fn default_plugin_compatibility() -> HashMap<String, PluginCompatibilityInfo> {
        let mut compatibility = HashMap::new();

        // Core pytest plugins - fully compatible
        compatibility.insert(
            "pytest-cov".to_string(),
            PluginCompatibilityInfo {
                status: CompatibilityStatus::FullyCompatible,
                veri_versions: vec!["*".to_string()],
                python_versions: vec![">=3.8".to_string()],
                notes: Some("Fully supported with incremental coverage".to_string()),
                fallback_required: false,
                mutates_collection: false,
                known_issues: Vec::new(),
            },
        );

        compatibility.insert(
            "pytest-mock".to_string(),
            PluginCompatibilityInfo {
                status: CompatibilityStatus::FullyCompatible,
                veri_versions: vec!["*".to_string()],
                python_versions: vec![">=3.8".to_string()],
                notes: Some("Mock fixtures work normally".to_string()),
                fallback_required: false,
                mutates_collection: false,
                known_issues: Vec::new(),
            },
        );

        compatibility.insert(
            "pytest-asyncio".to_string(),
            PluginCompatibilityInfo {
                status: CompatibilityStatus::FullyCompatible,
                veri_versions: vec!["*".to_string()],
                python_versions: vec![">=3.8".to_string()],
                notes: Some("Async test execution supported".to_string()),
                fallback_required: false,
                mutates_collection: false,
                known_issues: Vec::new(),
            },
        );

        // Plugins that need fallback
        compatibility.insert(
            "pytest-xdist".to_string(),
            PluginCompatibilityInfo {
                status: CompatibilityStatus::Incompatible,
                veri_versions: vec!["*".to_string()],
                python_versions: vec!["*".to_string()],
                notes: Some(
                    "Conflicts with veri's own parallelization - use veri's --workers instead"
                        .to_string(),
                ),
                fallback_required: true,
                mutates_collection: false,
                known_issues: vec![
                    "Parallel execution conflicts".to_string(),
                    "Worker management interference".to_string(),
                ],
            },
        );

        compatibility.insert(
            "pytest-testmon".to_string(),
            PluginCompatibilityInfo {
                status: CompatibilityStatus::Incompatible,
                veri_versions: vec!["*".to_string()],
                python_versions: vec!["*".to_string()],
                notes: Some(
                    "Conflicts with veri's impact analysis - veri provides better impact detection"
                        .to_string(),
                ),
                fallback_required: true,
                mutates_collection: false,
                known_issues: vec![
                    "Impact analysis conflicts".to_string(),
                    "File monitoring interference".to_string(),
                ],
            },
        );

        // Collection-mutating plugins
        compatibility.insert(
            "pytest-randomly".to_string(),
            PluginCompatibilityInfo {
                status: CompatibilityStatus::NeedsSpecialHandling,
                veri_versions: vec!["*".to_string()],
                python_versions: vec!["*".to_string()],
                notes: Some("Test ordering changes may affect impact analysis".to_string()),
                fallback_required: false,
                mutates_collection: true,
                known_issues: vec!["Test order randomization affects caching".to_string()],
            },
        );

        compatibility.insert(
            "pytest-order".to_string(),
            PluginCompatibilityInfo {
                status: CompatibilityStatus::NeedsSpecialHandling,
                veri_versions: vec!["*".to_string()],
                python_versions: vec!["*".to_string()],
                notes: Some("Test ordering changes may affect scheduling".to_string()),
                fallback_required: false,
                mutates_collection: true,
                known_issues: vec![
                    "Custom test ordering affects scheduling optimization".to_string()
                ],
            },
        );

        // Framework plugins - generally compatible
        compatibility.insert(
            "pytest-django".to_string(),
            PluginCompatibilityInfo {
                status: CompatibilityStatus::FullyCompatible,
                veri_versions: vec!["*".to_string()],
                python_versions: vec![">=3.8".to_string()],
                notes: Some("Django test database handling supported".to_string()),
                fallback_required: false,
                mutates_collection: false,
                known_issues: Vec::new(),
            },
        );

        compatibility.insert(
            "pytest-flask".to_string(),
            PluginCompatibilityInfo {
                status: CompatibilityStatus::FullyCompatible,
                veri_versions: vec!["*".to_string()],
                python_versions: vec![">=3.8".to_string()],
                notes: Some("Flask test client fixtures work normally".to_string()),
                fallback_required: false,
                mutates_collection: false,
                known_issues: Vec::new(),
            },
        );

        compatibility
    }

    /// Check if the current environment is in the compatibility matrix
    pub fn check_current_environment(
        &self,
        _worker: &PythonWorker,
    ) -> Result<EnvironmentCompatibility> {
        // For now, use simple detection since we don't have access to worker internals
        let python_version = self.detect_python_version();
        let os = self.detect_os();
        let env_type = self.detect_environment_type_simple();

        let python_supported = self
            .python_versions
            .iter()
            .any(|v| python_version.starts_with(v));

        let os_supported = self.operating_systems.contains(&os);

        let env_supported = self.environment_types.contains(&env_type);

        Ok(EnvironmentCompatibility {
            python_version: python_version.clone(),
            python_supported,
            os: os.clone(),
            os_supported,
            environment_type: env_type.clone(),
            environment_supported: env_supported,
            overall_supported: python_supported && os_supported && env_supported,
            warnings: self.generate_environment_warnings(&python_version, &os, &env_type),
        })
    }

    /// Check plugin compatibility and determine if fallback is needed
    pub fn check_plugin_compatibility(&self, plugins: &[String]) -> PluginCompatibilityCheck {
        let mut results = HashMap::new();
        let mut needs_fallback = false;
        let mut collection_mutating = false;
        let mut warnings = Vec::new();

        for plugin in plugins {
            let plugin_name = self.extract_plugin_name(plugin);

            if let Some(info) = self.plugin_compatibility.get(&plugin_name) {
                let compatible = match info.status {
                    CompatibilityStatus::FullyCompatible => true,
                    CompatibilityStatus::NeedsSpecialHandling => true,
                    CompatibilityStatus::Incompatible => false,
                    CompatibilityStatus::Unknown => true, // Assume compatible but warn
                };

                if info.fallback_required {
                    needs_fallback = true;
                }

                if info.mutates_collection {
                    collection_mutating = true;
                    warnings.push(format!(
                        "Plugin '{}' may affect test collection/ordering - caching may be less effective",
                        plugin_name
                    ));
                }

                for issue in &info.known_issues {
                    warnings.push(format!("Plugin '{}': {}", plugin_name, issue));
                }

                results.insert(plugin_name.clone(), (compatible, info.clone()));
            } else {
                // Unknown plugin - assume compatible but warn
                warnings.push(format!(
                    "Plugin '{}' compatibility unknown - assuming compatible. Report issues at https://github.com/dennis8/veri/issues",
                    plugin_name
                ));

                results.insert(
                    plugin_name.clone(),
                    (
                        true,
                        PluginCompatibilityInfo {
                            status: CompatibilityStatus::Unknown,
                            veri_versions: vec!["*".to_string()],
                            python_versions: vec!["*".to_string()],
                            notes: Some("Unknown plugin - compatibility not verified".to_string()),
                            fallback_required: false,
                            mutates_collection: false,
                            known_issues: Vec::new(),
                        },
                    ),
                );
            }
        }

        PluginCompatibilityCheck {
            results,
            needs_fallback,
            collection_mutating,
            warnings,
        }
    }

    /// Extract plugin name from version specification
    fn extract_plugin_name(&self, plugin_spec: &str) -> String {
        plugin_spec
            .split(&['=', '>', '<', '~', '!', '['][..])
            .next()
            .unwrap_or(plugin_spec)
            .to_string()
    }

    /// Detect current operating system
    fn detect_os(&self) -> String {
        if cfg!(target_os = "windows") {
            "windows".to_string()
        } else if cfg!(target_os = "macos") {
            "macos".to_string()
        } else if cfg!(target_os = "linux") {
            "linux".to_string()
        } else {
            "unknown".to_string()
        }
    }

    /// Detect environment type (simplified)
    fn detect_environment_type_simple(&self) -> String {
        if std::env::var("VIRTUAL_ENV").is_ok() {
            return "venv".to_string();
        }

        if std::env::var("CONDA_DEFAULT_ENV").is_ok() {
            return "conda".to_string();
        }

        // Check current directory for environment indicators
        let current_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

        if current_dir.join("uv.lock").exists() {
            return "uv".to_string();
        }

        if current_dir.join("poetry.lock").exists() {
            return "poetry".to_string();
        }

        "system".to_string()
    }

    /// Detect environment type in a specific directory (checks for common lock files)
    pub fn detect_environment_type<P: AsRef<Path>>(&self, dir: P) -> Result<String> {
        let dir = dir.as_ref();
        if dir.join("uv.lock").exists() {
            return Ok("uv".to_string());
        }
        if dir.join("poetry.lock").exists() {
            return Ok("poetry".to_string());
        }
        // Fall back to environment variables if present
        if std::env::var("VIRTUAL_ENV").is_ok() {
            return Ok("venv".to_string());
        }
        if std::env::var("CONDA_DEFAULT_ENV").is_ok() {
            return Ok("conda".to_string());
        }
        Ok("system".to_string())
    }

    /// Detect Python version (simplified)
    fn detect_python_version(&self) -> String {
        // Try to get from environment or use a reasonable default
        std::env::var("PYTHON_VERSION").unwrap_or_else(|_| {
            format!(
                "{}.{}",
                std::env::var("PYTHON_VERSION_MAJOR").unwrap_or("3".to_string()),
                std::env::var("PYTHON_VERSION_MINOR").unwrap_or("11".to_string())
            )
        })
    }

    /// Generate warnings for environment compatibility
    fn generate_environment_warnings(
        &self,
        python_version: &str,
        os: &str,
        env_type: &str,
    ) -> Vec<String> {
        let mut warnings = Vec::new();

        if !self
            .python_versions
            .iter()
            .any(|v| python_version.starts_with(v))
        {
            warnings.push(format!(
                "Python {} is not in the tested compatibility matrix. Supported versions: {}",
                python_version,
                self.python_versions.join(", ")
            ));
        }

        if !self.operating_systems.contains(&os.to_string()) {
            warnings.push(format!(
                "Operating system '{}' is not in the tested compatibility matrix. Supported: {}",
                os,
                self.operating_systems.join(", ")
            ));
        }

        if !self.environment_types.contains(&env_type.to_string()) {
            warnings.push(format!(
                "Environment type '{}' is not in the tested compatibility matrix. Supported: {}",
                env_type,
                self.environment_types.join(", ")
            ));
        }

        warnings
    }

    /// Generate compatibility report
    pub fn generate_report(
        &self,
        worker: &PythonWorker,
        plugins: &[String],
    ) -> Result<CompatibilityReport> {
        let environment = self.check_current_environment(worker)?;
        let plugin_check = self.check_plugin_compatibility(plugins);

        let recommendations = self.generate_recommendations(&environment, &plugin_check);

        Ok(CompatibilityReport {
            environment,
            plugin_check,
            recommendations,
        })
    }

    /// Generate recommendations based on compatibility check
    fn generate_recommendations(
        &self,
        env: &EnvironmentCompatibility,
        plugins: &PluginCompatibilityCheck,
    ) -> Vec<String> {
        let mut recommendations = Vec::new();

        if !env.overall_supported {
            if !env.python_supported {
                recommendations.push(format!(
                    "Consider upgrading to a supported Python version: {}",
                    self.python_versions.join(", ")
                ));
            }

            if !env.os_supported {
                recommendations.push(format!(
                    "Your OS '{}' may have limited support. Consider testing on: {}",
                    env.os,
                    self.operating_systems.join(", ")
                ));
            }

            if !env.environment_supported {
                recommendations.push(format!(
                    "Consider using a supported environment type: {}",
                    self.environment_types.join(", ")
                ));
            }
        }

        if plugins.needs_fallback {
            recommendations.push(
                "Some plugins require fallback to pytest engine. Consider using --engine pytest"
                    .to_string(),
            );
        }

        if plugins.collection_mutating {
            recommendations.push(
                "Collection-mutating plugins detected. Cache effectiveness may be reduced"
                    .to_string(),
            );
        }

        if !plugins.warnings.is_empty() {
            recommendations.push(
                "Check plugin compatibility warnings above and consider updating or replacing problematic plugins".to_string()
            );
        }

        recommendations
    }
}

/// Plugin compatibility status
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CompatibilityStatus {
    /// Plugin works perfectly with veri
    FullyCompatible,
    /// Plugin works but may need special handling
    NeedsSpecialHandling,
    /// Plugin conflicts with veri and requires fallback to pytest
    Incompatible,
    /// Plugin compatibility is unknown
    Unknown,
}

/// Information about a plugin's compatibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCompatibilityInfo {
    pub status: CompatibilityStatus,
    pub veri_versions: Vec<String>,
    pub python_versions: Vec<String>,
    pub notes: Option<String>,
    pub fallback_required: bool,
    pub mutates_collection: bool,
    pub known_issues: Vec<String>,
}

/// Result of environment compatibility check
#[derive(Debug, Clone)]
pub struct EnvironmentCompatibility {
    pub python_version: String,
    pub python_supported: bool,
    pub os: String,
    pub os_supported: bool,
    pub environment_type: String,
    pub environment_supported: bool,
    pub overall_supported: bool,
    pub warnings: Vec<String>,
}

/// Result of plugin compatibility check
#[derive(Debug, Clone)]
pub struct PluginCompatibilityCheck {
    pub results: HashMap<String, (bool, PluginCompatibilityInfo)>,
    pub needs_fallback: bool,
    pub collection_mutating: bool,
    pub warnings: Vec<String>,
}

/// Full compatibility report
#[derive(Debug, Clone)]
pub struct CompatibilityReport {
    pub environment: EnvironmentCompatibility,
    pub plugin_check: PluginCompatibilityCheck,
    pub recommendations: Vec<String>,
}

impl CompatibilityReport {
    /// Print a formatted compatibility report
    pub fn print_report(&self, use_color: bool) {
        let _green = if use_color { "\x1b[32m" } else { "" };
        let yellow = if use_color { "\x1b[33m" } else { "" };
        let _red = if use_color { "\x1b[31m" } else { "" };
        let reset = if use_color { "\x1b[0m" } else { "" };

        println!("🔍 Compatibility Report");
        println!("========================");
        println!();

        // Environment compatibility
        println!("Environment:");
        let env_status = if self.environment.overall_supported {
            "✅ Fully Supported".to_string()
        } else {
            "⚠️  Limited Support".to_string()
        };
        println!("  Status: {}", env_status);
        println!(
            "  Python: {} ({})",
            self.environment.python_version,
            if self.environment.python_supported {
                "✅"
            } else {
                "❌"
            }
        );
        println!(
            "  OS: {} ({})",
            self.environment.os,
            if self.environment.os_supported {
                "✅"
            } else {
                "❌"
            }
        );
        println!(
            "  Environment: {} ({})",
            self.environment.environment_type,
            if self.environment.environment_supported {
                "✅"
            } else {
                "❌"
            }
        );

        if !self.environment.warnings.is_empty() {
            println!();
            println!("  Environment Warnings:");
            for warning in &self.environment.warnings {
                println!("    {}⚠️  {}{}", yellow, warning, reset);
            }
        }

        println!();

        // Plugin compatibility
        println!("Plugins:");
        if self.plugin_check.results.is_empty() {
            println!("  No plugins detected");
        } else {
            for (plugin, (_compatible, info)) in &self.plugin_check.results {
                let status_icon = match info.status {
                    CompatibilityStatus::FullyCompatible => "✅",
                    CompatibilityStatus::NeedsSpecialHandling => "⚠️",
                    CompatibilityStatus::Incompatible => "❌",
                    CompatibilityStatus::Unknown => "❓",
                };

                println!("  {} {}", status_icon, plugin);

                if let Some(notes) = &info.notes {
                    println!("     {}", notes);
                }

                if info.fallback_required {
                    println!(
                        "     {}📋 Requires fallback to pytest engine{}",
                        yellow, reset
                    );
                }

                if info.mutates_collection {
                    println!(
                        "     {}🔄 May affect test collection/ordering{}",
                        yellow, reset
                    );
                }
            }
        }

        if !self.plugin_check.warnings.is_empty() {
            println!();
            println!("  Plugin Warnings:");
            for warning in &self.plugin_check.warnings {
                println!("    {}⚠️  {}{}", yellow, warning, reset);
            }
        }

        // Recommendations
        if !self.recommendations.is_empty() {
            println!();
            println!("Recommendations:");
            for recommendation in &self.recommendations {
                println!("  💡 {}", recommendation);
            }
        }

        // Final summary
        println!();
        if self.environment.overall_supported && !self.plugin_check.needs_fallback {
            println!("✅ Your environment is fully compatible with veri");
        } else if self.plugin_check.needs_fallback {
            println!("⚠️  Some plugins require fallback mode - use --engine pytest for full compatibility");
        } else {
            println!("⚠️  Environment has compatibility limitations");
        }

        println!();
        println!("📖 For more details: https://docs.veri.dev/compatibility");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_compatibility_matrix() {
        let matrix = CompatibilityMatrix::default();
        assert!(!matrix.python_versions.is_empty());
        assert!(!matrix.operating_systems.is_empty());
        assert!(!matrix.plugin_compatibility.is_empty());
    }

    #[test]
    fn test_plugin_name_extraction() {
        let matrix = CompatibilityMatrix::default();
        assert_eq!(
            matrix.extract_plugin_name("pytest-cov==5.0.0"),
            "pytest-cov"
        );
        assert_eq!(
            matrix.extract_plugin_name("pytest-mock>=3.0"),
            "pytest-mock"
        );
        assert_eq!(matrix.extract_plugin_name("simple-plugin"), "simple-plugin");
    }

    #[test]
    fn test_plugin_compatibility_check() {
        let matrix = CompatibilityMatrix::default();
        let plugins = vec![
            "pytest-cov".to_string(),
            "pytest-xdist".to_string(),
            "unknown-plugin".to_string(),
        ];

        let check = matrix.check_plugin_compatibility(&plugins);

        // pytest-cov should be compatible
        assert!(check.results.get("pytest-cov").unwrap().0);

        // pytest-xdist should be incompatible and require fallback
        assert!(!check.results.get("pytest-xdist").unwrap().0);
        assert!(check.needs_fallback);

        // unknown-plugin should be assumed compatible but generate warning
        assert!(check.results.get("unknown-plugin").unwrap().0);
        assert!(!check.warnings.is_empty());
    }

    #[test]
    fn test_os_detection() {
        let matrix = CompatibilityMatrix::default();
        let os = matrix.detect_os();
        assert!(["windows", "macos", "linux", "unknown"].contains(&os.as_str()));
    }

    #[test]
    fn test_environment_type_detection() -> Result<()> {
        let matrix = CompatibilityMatrix::default();
        let temp_dir = TempDir::new()?;

        // Test uv.lock detection
        std::fs::write(temp_dir.path().join("uv.lock"), "")?;
        let env_type = matrix.detect_environment_type(temp_dir.path())?;
        assert_eq!(env_type, "uv");

        Ok(())
    }
}
