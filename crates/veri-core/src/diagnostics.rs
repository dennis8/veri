//! Enhanced error messages and diagnostics for veri
//! 
//! This module provides user-friendly error messages with actionable guidance,
//! helping users understand and resolve common issues.

use std::fmt;
use anyhow::Result;

/// Common error scenarios that users encounter
#[derive(Debug, Clone)]
pub enum VeriDiagnostic {
    NoTestsFound {
        filters_applied: bool,
        keyword_filter: Option<String>,
        marker_filter: Option<String>,
        paths_provided: bool,
    },
    ImportGraphBuildFailed {
        error_count: usize,
        syntax_errors: Vec<String>,
        missing_files: Vec<String>,
    },
    DynamicImportDetected {
        from_module: String,
        import_expression: String,
        broadening_triggered: bool,
    },
    PluginIncompatible {
        plugin_name: String,
        version: String,
        reason: String,
        fallback_suggested: bool,
    },
    ConfigurationError {
        config_path: String,
        error_type: ConfigErrorType,
        suggestions: Vec<String>,
    },
    SelectionBroadened {
        original_count: usize,
        total_count: usize,
        threshold: f64,
        reason: String,
    },
    CacheMiss {
        reason: CacheMissReason,
        rebuild_time_estimate: Option<String>,
    },
    PythonEnvironmentIssue {
        issue_type: PythonIssueType,
        current_python: String,
        suggestions: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub enum ConfigErrorType {
    InvalidSyntax(String),
    ConflictingOptions(String, String),
    InvalidValue(String, String),
    MissingRequired(String),
}

#[derive(Debug, Clone)]
pub enum CacheMissReason {
    FirstRun,
    ConfigChanged,
    PythonVersionChanged,
    DependenciesChanged,
    FileSystemChanged,
    CacheCorrupted,
}

#[derive(Debug, Clone)]
pub enum PythonIssueType {
    NotFound,
    VersionMismatch(String, String), // expected, actual
    MissingDependencies(Vec<String>),
    PermissionDenied,
    VirtualEnvNotActivated,
}

impl VeriDiagnostic {
    /// Format the diagnostic as a user-friendly error message
    pub fn format_message(&self) -> String {
        match self {
            VeriDiagnostic::NoTestsFound {
                filters_applied,
                keyword_filter,
                marker_filter,
                paths_provided,
            } => {
                let mut message = String::from("❌ No tests found matching your criteria\n\n");
                
                message.push_str("Possible reasons:\n");
                
                if *filters_applied {
                    if let Some(keyword) = keyword_filter {
                        message.push_str(&format!("  • Keyword filter '{}' too restrictive\n", keyword));
                    }
                    if let Some(marker) = marker_filter {
                        message.push_str(&format!("  • Marker filter '{}' excludes all tests\n", marker));
                    }
                    if *paths_provided {
                        message.push_str("  • Specified paths don't contain test files\n");
                    }
                } else {
                    message.push_str("  • No test files found in current directory\n");
                    message.push_str("  • Test files don't follow naming convention\n");
                    message.push_str("  • Import errors preventing test collection\n");
                }
                
                message.push_str("\nSuggestions:\n");
                message.push_str("  • Run 'veri --explain' to see selection logic\n");
                message.push_str("  • Check test file naming (test_*.py or *_test.py)\n");
                message.push_str("  • Verify Python path and dependencies\n");
                if *filters_applied {
                    message.push_str("  • Try removing filters (-k, -m, paths) to see all tests\n");
                }
                
                message.push_str("\nFor more help: https://docs.veri.dev/troubleshooting#no-tests-found");
                message
            }
            
            VeriDiagnostic::ImportGraphBuildFailed {
                error_count,
                syntax_errors,
                missing_files,
            } => {
                let mut message = String::from("⚠️  Import graph build encountered issues\n\n");
                
                message.push_str(&format!("Encountered {} errors during analysis:\n", error_count));
                
                if !syntax_errors.is_empty() {
                    message.push_str("\nSyntax errors:\n");
                    for error in syntax_errors.iter().take(5) {
                        message.push_str(&format!("  • {}\n", error));
                    }
                    if syntax_errors.len() > 5 {
                        message.push_str(&format!("  • ... and {} more\n", syntax_errors.len() - 5));
                    }
                }
                
                if !missing_files.is_empty() {
                    message.push_str("\nMissing files:\n");
                    for file in missing_files.iter().take(5) {
                        message.push_str(&format!("  • {}\n", file));
                    }
                    if missing_files.len() > 5 {
                        message.push_str(&format!("  • ... and {} more\n", missing_files.len() - 5));
                    }
                }
                
                message.push_str("\nImpact: Some tests may be selected unnecessarily (broadening for safety)\n");
                message.push_str("\nSuggestions:\n");
                message.push_str("  • Fix syntax errors in Python files\n");
                message.push_str("  • Ensure all imported modules are available\n");
                message.push_str("  • Consider adding problematic files to .veriignore\n");
                message.push_str("  • Run with --engine pytest to bypass impact analysis\n");
                
                message
            }
            
            VeriDiagnostic::DynamicImportDetected {
                from_module,
                import_expression,
                broadening_triggered,
            } => {
                let mut message = String::from("🔍 Dynamic import detected\n\n");
                
                message.push_str(&format!("Module: {}\n", from_module));
                message.push_str(&format!("Expression: {}\n", import_expression));
                
                if *broadening_triggered {
                    message.push_str("\n⚠️  Selection broadened to all tests for safety\n");
                    message.push_str("\nWhy: Dynamic imports can't be statically analyzed, so veri runs all tests\n");
                    message.push_str("to ensure nothing is missed.\n");
                    
                    message.push_str("\nTo avoid broadening:\n");
                    message.push_str("  • Replace dynamic imports with static imports where possible\n");
                    message.push_str("  • Use --engine pytest to bypass impact analysis\n");
                    message.push_str("  • Add the module to .veriignore if it's not test-related\n");
                } else {
                    message.push_str("\nNote: Dynamic import detected but not affecting current selection\n");
                }
                
                message
            }
            
            VeriDiagnostic::PluginIncompatible {
                plugin_name,
                version,
                reason,
                fallback_suggested,
            } => {
                let mut message = String::from("🔌 Plugin compatibility issue\n\n");
                
                message.push_str(&format!("Plugin: {} ({})\n", plugin_name, version));
                message.push_str(&format!("Issue: {}\n", reason));
                
                if *fallback_suggested {
                    message.push_str("\n✨ Automatic fallback activated\n");
                    message.push_str("Veri will use --engine pytest for this run to ensure compatibility.\n");
                    
                    message.push_str("\nTo resolve:\n");
                    message.push_str("  • Update to a compatible plugin version\n");
                    message.push_str("  • Check veri plugin compatibility list\n");
                    message.push_str("  • Remove problematic plugins if not needed\n");
                } else {
                    message.push_str("\nSuggestions:\n");
                    message.push_str("  • Try running with --engine pytest\n");
                    message.push_str("  • Update plugin to a compatible version\n");
                    message.push_str("  • Disable plugin temporarily\n");
                }
                
                message.push_str("\nCompatibility guide: https://docs.veri.dev/plugins#compatibility");
                message
            }
            
            VeriDiagnostic::ConfigurationError {
                config_path,
                error_type,
                suggestions,
            } => {
                let mut message = String::from("⚙️  Configuration error\n\n");
                
                message.push_str(&format!("File: {}\n", config_path));
                
                match error_type {
                    ConfigErrorType::InvalidSyntax(details) => {
                        message.push_str(&format!("Error: Invalid TOML syntax\n{}\n", details));
                    }
                    ConfigErrorType::ConflictingOptions(opt1, opt2) => {
                        message.push_str(&format!("Error: Conflicting options '{}' and '{}'\n", opt1, opt2));
                    }
                    ConfigErrorType::InvalidValue(option, value) => {
                        message.push_str(&format!("Error: Invalid value '{}' for option '{}'\n", value, option));
                    }
                    ConfigErrorType::MissingRequired(option) => {
                        message.push_str(&format!("Error: Missing required option '{}'\n", option));
                    }
                }
                
                if !suggestions.is_empty() {
                    message.push_str("\nSuggestions:\n");
                    for suggestion in suggestions {
                        message.push_str(&format!("  • {}\n", suggestion));
                    }
                }
                
                message.push_str("\nConfiguration reference: https://docs.veri.dev/config");
                message
            }
            
            VeriDiagnostic::SelectionBroadened {
                original_count,
                total_count,
                threshold,
                reason,
            } => {
                let mut message = String::from("📈 Test selection broadened\n\n");
                
                message.push_str(&format!("Impact analysis selected {} of {} tests ({:.1}%)\n", 
                    original_count, total_count, 
                    (*original_count as f64 / *total_count as f64) * 100.0));
                message.push_str(&format!("Threshold: {:.1}%\n", threshold * 100.0));
                message.push_str(&format!("Reason: {}\n", reason));
                
                message.push_str("\n🚀 Running all tests for optimal performance\n");
                message.push_str("\nWhy: When most tests would run anyway, it's faster to run them all\n");
                message.push_str("without the overhead of selective execution.\n");
                
                message.push_str("\nTo avoid broadening:\n");
                message.push_str("  • Make smaller, more focused changes\n");
                message.push_str("  • Increase threshold in configuration\n");
                message.push_str("  • Use test filters (-k, -m) to narrow selection\n");
                
                message
            }
            
            VeriDiagnostic::CacheMiss {
                reason,
                rebuild_time_estimate,
            } => {
                let mut message = String::from("🔄 Cache miss - rebuilding analysis\n\n");
                
                match reason {
                    CacheMissReason::FirstRun => {
                        message.push_str("Reason: First run in this project\n");
                    }
                    CacheMissReason::ConfigChanged => {
                        message.push_str("Reason: Configuration changed\n");
                    }
                    CacheMissReason::PythonVersionChanged => {
                        message.push_str("Reason: Python version changed\n");
                    }
                    CacheMissReason::DependenciesChanged => {
                        message.push_str("Reason: Dependencies changed (detected via uv.lock)\n");
                    }
                    CacheMissReason::FileSystemChanged => {
                        message.push_str("Reason: File system structure changed\n");
                    }
                    CacheMissReason::CacheCorrupted => {
                        message.push_str("Reason: Cache files corrupted or invalid\n");
                    }
                }
                
                if let Some(estimate) = rebuild_time_estimate {
                    message.push_str(&format!("Estimated rebuild time: {}\n", estimate));
                }
                
                message.push_str("\nNote: Subsequent runs will be much faster using cached analysis\n");
                
                message
            }
            
            VeriDiagnostic::PythonEnvironmentIssue {
                issue_type,
                current_python,
                suggestions,
            } => {
                let mut message = String::from("🐍 Python environment issue\n\n");
                
                match issue_type {
                    PythonIssueType::NotFound => {
                        message.push_str("Error: Python interpreter not found\n");
                    }
                    PythonIssueType::VersionMismatch(expected, actual) => {
                        message.push_str(&format!("Error: Python version mismatch\n"));
                        message.push_str(&format!("Expected: {}\n", expected));
                        message.push_str(&format!("Actual: {}\n", actual));
                    }
                    PythonIssueType::MissingDependencies(deps) => {
                        message.push_str("Error: Missing required dependencies\n");
                        for dep in deps {
                            message.push_str(&format!("  • {}\n", dep));
                        }
                    }
                    PythonIssueType::PermissionDenied => {
                        message.push_str("Error: Permission denied accessing Python environment\n");
                    }
                    PythonIssueType::VirtualEnvNotActivated => {
                        message.push_str("Warning: Virtual environment not activated\n");
                    }
                }
                
                message.push_str(&format!("Current Python: {}\n", current_python));
                
                if !suggestions.is_empty() {
                    message.push_str("\nSuggestions:\n");
                    for suggestion in suggestions {
                        message.push_str(&format!("  • {}\n", suggestion));
                    }
                }
                
                message
            }
        }
    }

    /// Get the appropriate exit code for this diagnostic
    pub fn exit_code(&self) -> i32 {
        match self {
            VeriDiagnostic::NoTestsFound { .. } => 4, // UsageError
            VeriDiagnostic::ConfigurationError { .. } => 4, // UsageError
            VeriDiagnostic::PythonEnvironmentIssue { .. } => 3, // InternalError
            VeriDiagnostic::PluginIncompatible { .. } => 3, // InternalError
            _ => 0, // These are warnings/info, not errors
        }
    }

    /// Check if this diagnostic should be treated as an error (non-zero exit)
    pub fn is_error(&self) -> bool {
        self.exit_code() != 0
    }

    /// Get a short title for this diagnostic
    pub fn title(&self) -> &'static str {
        match self {
            VeriDiagnostic::NoTestsFound { .. } => "No Tests Found",
            VeriDiagnostic::ImportGraphBuildFailed { .. } => "Import Analysis Issues",
            VeriDiagnostic::DynamicImportDetected { .. } => "Dynamic Import Detected",
            VeriDiagnostic::PluginIncompatible { .. } => "Plugin Incompatible",
            VeriDiagnostic::ConfigurationError { .. } => "Configuration Error",
            VeriDiagnostic::SelectionBroadened { .. } => "Selection Broadened",
            VeriDiagnostic::CacheMiss { .. } => "Cache Miss",
            VeriDiagnostic::PythonEnvironmentIssue { .. } => "Python Environment Issue",
        }
    }
}

impl fmt::Display for VeriDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_message())
    }
}

/// Helper functions for creating common diagnostics
impl VeriDiagnostic {
    /// Create a "no tests found" diagnostic based on current filters
    pub fn no_tests_found(
        keyword: Option<&str>,
        marker: Option<&str>,
        paths: &[String],
    ) -> Self {
        let filters_applied = keyword.is_some() || marker.is_some() || !paths.is_empty();
        
        VeriDiagnostic::NoTestsFound {
            filters_applied,
            keyword_filter: keyword.map(String::from),
            marker_filter: marker.map(String::from),
            paths_provided: !paths.is_empty(),
        }
    }

    /// Create a dynamic import diagnostic
    pub fn dynamic_import_detected(
        from_module: impl Into<String>,
        import_expr: impl Into<String>,
        broadened: bool,
    ) -> Self {
        VeriDiagnostic::DynamicImportDetected {
            from_module: from_module.into(),
            import_expression: import_expr.into(),
            broadening_triggered: broadened,
        }
    }

    /// Create a selection broadened diagnostic
    pub fn selection_broadened(
        original: usize,
        total: usize,
        threshold: f64,
        reason: impl Into<String>,
    ) -> Self {
        VeriDiagnostic::SelectionBroadened {
            original_count: original,
            total_count: total,
            threshold,
            reason: reason.into(),
        }
    }
}

/// Diagnostic reporter for accumulating and displaying diagnostics
pub struct DiagnosticReporter {
    diagnostics: Vec<VeriDiagnostic>,
    quiet: bool,
}

impl DiagnosticReporter {
    /// Create a new diagnostic reporter
    pub fn new(quiet: bool) -> Self {
        Self {
            diagnostics: Vec::new(),
            quiet,
        }
    }

    /// Add a diagnostic
    pub fn add(&mut self, diagnostic: VeriDiagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Report all diagnostics to stderr
    pub fn report_all(&self) -> Result<()> {
        for diagnostic in &self.diagnostics {
            if !self.quiet || diagnostic.is_error() {
                eprintln!("{}", diagnostic.format_message());
                eprintln!(); // Empty line for separation
            }
        }
        Ok(())
    }

    /// Get the highest exit code from all diagnostics
    pub fn max_exit_code(&self) -> i32 {
        self.diagnostics
            .iter()
            .map(|d| d.exit_code())
            .max()
            .unwrap_or(0)
    }

    /// Check if any diagnostic is an error
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.is_error())
    }

    /// Get count of errors vs warnings
    pub fn counts(&self) -> (usize, usize) {
        let errors = self.diagnostics.iter().filter(|d| d.is_error()).count();
        let warnings = self.diagnostics.len() - errors;
        (errors, warnings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_tests_found_formatting() {
        let diagnostic = VeriDiagnostic::no_tests_found(
            Some("test_example"),
            None,
            &[],
        );
        
        let message = diagnostic.format_message();
        assert!(message.contains("No tests found"));
        assert!(message.contains("test_example"));
        assert!(message.contains("--explain"));
    }

    #[test]
    fn test_diagnostic_exit_codes() {
        let no_tests = VeriDiagnostic::no_tests_found(None, None, &[]);
        assert_eq!(no_tests.exit_code(), 4);
        assert!(no_tests.is_error());

        let broadened = VeriDiagnostic::selection_broadened(15, 20, 0.6, "threshold");
        assert_eq!(broadened.exit_code(), 0);
        assert!(!broadened.is_error());
    }

    #[test]
    fn test_diagnostic_reporter() {
        let mut reporter = DiagnosticReporter::new(false);
        
        reporter.add(VeriDiagnostic::no_tests_found(None, None, &[]));
        reporter.add(VeriDiagnostic::selection_broadened(15, 20, 0.6, "test"));
        
        assert_eq!(reporter.max_exit_code(), 4);
        assert!(reporter.has_errors());
        
        let (errors, warnings) = reporter.counts();
        assert_eq!(errors, 1);
        assert_eq!(warnings, 1);
    }
}