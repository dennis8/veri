use anyhow::Result;
use veri_core::compatibility::CompatibilityMatrix;
use veri_core::config::Config;
use veri_core::diagnostics::{DiagnosticReporter, VeriDiagnostic};
use veri_core::python_worker::PythonWorker;
use veri_core::security::{SecurityConfig, SecurityScanner};

use crate::cli::{Cli, ExitCode};
use super::telemetry::TelemetryService;

/// Result of environment validation
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Whether environment is valid for veri engine
    pub valid: bool,
    /// If Some, we should exit early with this code (e.g., fallback to pytest)
    pub fallback_exit: Option<ExitCode>,
    /// Diagnostic messages from validation
    pub diagnostics: Vec<String>,
}

/// Trait for orchestrating environment validation
pub trait ValidationOrchestrationService: Send + Sync {
    fn validate_environment(
        &self,
        cli: &Cli,
        config: &Config,
        worker: &PythonWorker,
        compatibility_matrix: &CompatibilityMatrix,
        security_config: &SecurityConfig,
        telemetry: &mut TelemetryService,
        diagnostics: &mut DiagnosticReporter,
    ) -> Result<ValidationResult>;
}

/// Default implementation for environment validation
pub struct DefaultValidationService;

impl DefaultValidationService {
    pub fn new() -> Self {
        Self
    }
}

impl ValidationOrchestrationService for DefaultValidationService {
    fn validate_environment(
        &self,
        cli: &Cli,
        config: &Config,
        worker: &PythonWorker,
        compatibility_matrix: &CompatibilityMatrix,
        security_config: &SecurityConfig,
        telemetry: &mut TelemetryService,
        diagnostics: &mut DiagnosticReporter,
    ) -> Result<ValidationResult> {
        // Step 1: Check compatibility and handle fallback
        let plugins = worker.get_pytest_plugins().unwrap_or_default();
        let compatibility_report = compatibility_matrix.generate_report(worker, &plugins)?;

        // If explicitly requested, print the compatibility report and exit
        if cli.compatibility_report {
            compatibility_report.print_report(!config.no_color());
            return Ok(ValidationResult {
                valid: false,
                fallback_exit: Some(ExitCode::Success),
                diagnostics: vec!["Compatibility report requested".to_string()],
            });
        }

        // Otherwise, print when verbose or issues detected
        if cli.verbose > 0
            || !compatibility_report.environment.overall_supported
            || compatibility_report.plugin_check.needs_fallback
        {
            compatibility_report.print_report(!config.no_color());
            println!();
        }

        // Auto-fallback to pytest if incompatible plugins detected
        if compatibility_report.plugin_check.needs_fallback && !cli.disable_allowlist {
            println!(
                "🔄 Automatically falling back to pytest engine due to plugin compatibility issues"
            );
            return Ok(ValidationResult {
                valid: false,
                fallback_exit: Some(ExitCode::Success),
                diagnostics: vec!["Fallback to pytest due to plugin incompatibility".to_string()],
            });
        }

        // Step 2: Check Python environment
        worker.check_environment(diagnostics)?;

        // Step 3: Validate plugins and run security checks
        if security_config.enforce_allowlist {
            println!("🔒 Validating pytest plugins...");
            match worker.get_pytest_plugins() {
                Ok(plugins) => {
                    let validation_result = security_config.validate_plugins(&plugins);

                    if validation_result.has_blocked_plugins() {
                        if !cli.disable_allowlist {
                            // Add diagnostic and exit when not overridden
                            diagnostics.add(VeriDiagnostic::PluginIncompatible {
                                plugin_name: validation_result.blocked.join(", "),
                                version: "unknown".to_string(),
                                reason: "Plugin not in allowlist".to_string(),
                                fallback_suggested: true,
                            });

                            if let Some(warning) = validation_result.get_warning_message() {
                                eprintln!("{}", warning);
                            }

                            println!(
                                "🚨 Blocked plugins detected. Use --disable-allowlist to override (not recommended)"
                            );
                            telemetry.record_error(veri_core::telemetry::ErrorCategory::PluginError);
                            return Ok(ValidationResult {
                                valid: false,
                                fallback_exit: Some(ExitCode::UsageError),
                                diagnostics: vec!["Blocked plugins detected".to_string()],
                            });
                        } else {
                            // Overridden: log a warning but do not add fatal diagnostic
                            if let Some(warning) = validation_result.get_warning_message() {
                                eprintln!("{}", warning);
                            }
                        }
                    } else {
                        println!(
                            "✅ All {} plugins are allowed",
                            validation_result.allowed.len()
                        );
                    }

                    // Run security scanner for additional warnings
                    let security_warnings = SecurityScanner::scan_plugins(&plugins);
                    for warning in &security_warnings {
                        println!("{}", warning.format(!config.no_color()));
                    }
                }
                Err(e) => {
                    println!("⚠️  Could not validate plugins: {}", e);
                    telemetry.record_error(veri_core::telemetry::ErrorCategory::PluginError);
                }
            }
        } else {
            println!("ℹ️  Plugin allowlist enforcement disabled");
        }

        Ok(ValidationResult {
            valid: true,
            fallback_exit: None,
            diagnostics: vec![],
        })
    }
}

#[cfg(test)]
mod testing {
    use super::*;
    use std::sync::Arc;

    /// Mock validation service for testing
    pub struct MockValidationService {
        pub result: Arc<std::sync::Mutex<ValidationResult>>,
    }

    impl MockValidationService {
        pub fn new(result: ValidationResult) -> Self {
            Self {
                result: Arc::new(std::sync::Mutex::new(result)),
            }
        }
    }

    impl ValidationOrchestrationService for MockValidationService {
        fn validate_environment(
            &self,
            _cli: &Cli,
            _config: &Config,
            _worker: &PythonWorker,
            _compatibility_matrix: &CompatibilityMatrix,
            _security_config: &SecurityConfig,
            _telemetry: &mut TelemetryService,
            _diagnostics: &mut DiagnosticReporter,
        ) -> Result<ValidationResult> {
            Ok(self
                .result
                .lock()
                .map_err(|e| anyhow::anyhow!("Mock lock failed: {}", e))?
                .clone())
        }
    }
}
