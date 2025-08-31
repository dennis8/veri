//! Impact analysis and test selection planner
//!
//! This module implements the planner logic for determining which tests
//! to run based on changed files and import dependencies.

use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::import_graph::{ImportsGraph, ModuleMap, ReverseDepsGraph};
use crate::python_worker::TestsIndex;

/// Test selection reason for explain mode
#[derive(Debug, Clone)]
pub enum SelectionReason {
    TestFileChanged {
        path: String,
    },
    SourceFileChanged {
        path: String,
        impacted_modules: Vec<String>,
    },
    ConftestChanged {
        path: String,
        scope: String,
    },
    PluginChanged {
        plugin: String,
    },
    DynamicImportDetected {
        from_module: String,
        reason: String,
    },
    ThresholdExceeded {
        percentage: f64,
        threshold: f64,
    },
    FullRun {
        reason: String,
    },
}

/// Test selection result
#[derive(Debug)]
pub struct TestSelection {
    pub selected_nodeids: Vec<String>,
    pub total_tests: usize,
    pub selection_reasons: Vec<SelectionReason>,
    pub should_broaden: bool,
    pub broaden_reason: Option<String>,
}

/// Configuration for the planner
#[derive(Debug)]
pub struct PlannerConfig {
    /// Threshold percentage for broadening selection (default: 60%)
    pub broaden_threshold: f64,
    /// Whether to enable dynamic import detection safety
    pub enable_dynamic_import_safety: bool,
    /// Whether to enable conftest scope detection
    pub enable_conftest_scope: bool,
}

impl PlannerConfig {
    /// Validate configuration values
    pub fn validate(&self) -> Result<()> {
        if self.broaden_threshold < 0.0 || self.broaden_threshold > 1.0 {
            return Err(anyhow::anyhow!(
                "broaden_threshold must be between 0.0 and 1.0 (fraction), got {}. \
                 Use 0.6 for 60%, not 60.",
                self.broaden_threshold
            ));
        }
        Ok(())
    }
}

impl Default for PlannerConfig {
    fn default() -> Self {
        Self {
            broaden_threshold: 0.6,
            enable_dynamic_import_safety: true,
            enable_conftest_scope: true,
        }
    }
}

/// Impact-aware test planner
pub struct TestPlanner {
    #[allow(dead_code)]
    work_dir: PathBuf,
    #[allow(dead_code)]
    cache_dir: PathBuf,
    config: PlannerConfig,
}

impl TestPlanner {
    /// Create a new test planner
    pub fn new(work_dir: impl Into<PathBuf>, cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            work_dir: work_dir.into(),
            cache_dir: cache_dir.into(),
            config: PlannerConfig::default(),
        }
    }

    /// Create with custom configuration
    pub fn with_config(
        work_dir: impl Into<PathBuf>,
        cache_dir: impl Into<PathBuf>,
        config: PlannerConfig,
    ) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            work_dir: work_dir.into(),
            cache_dir: cache_dir.into(),
            config,
        })
    }

    /// Plan test selection based on changed files
    pub fn plan_test_selection(
        &self,
        changed_files: &[String],
        tests_index: &TestsIndex,
        revdeps_graph: &ReverseDepsGraph,
        module_map: &ModuleMap,
        imports_graph: &ImportsGraph,
    ) -> Result<TestSelection> {
        let mut selected_nodeids = HashSet::new();
        let mut selection_reasons = Vec::new();
        let mut should_broaden = false;
        let mut broaden_reason = None;

        // If no files changed, run nothing (unless forced)
        if changed_files.is_empty() {
            return Ok(TestSelection {
                selected_nodeids: Vec::new(),
                total_tests: tests_index.tests.len(),
                selection_reasons,
                should_broaden: false,
                broaden_reason: None,
            });
        }

        // Convert file paths to module names for analysis
        let changed_modules = self.resolve_changed_modules(changed_files, module_map)?;

        // Apply invalidation rules
        for file_path in changed_files {
            // Rule 1: Test file changed → run its nodeids
            if self.is_test_file(file_path) {
                let test_nodeids = self.get_nodeids_for_test_file(file_path, tests_index);
                selected_nodeids.extend(test_nodeids.iter().cloned());
                selection_reasons.push(SelectionReason::TestFileChanged {
                    path: file_path.clone(),
                });
            }

            // Rule 2: conftest.py change → run tests under that directory
            if file_path.ends_with("conftest.py") && self.config.enable_conftest_scope {
                let scope_nodeids = self.get_nodeids_for_conftest_scope(file_path, tests_index);
                selected_nodeids.extend(scope_nodeids.iter().cloned());
                selection_reasons.push(SelectionReason::ConftestChanged {
                    path: file_path.clone(),
                    scope: self.get_conftest_scope(file_path),
                });
            }

            // Rule 3: Source file changed → run tests whose revdeps include that module
            if let Some(module_name) = changed_modules.get(file_path) {
                if let Some(reverse_deps) = revdeps_graph.reverse_deps.get(module_name) {
                    // Add direct test dependents
                    selected_nodeids.extend(reverse_deps.test_dependents.iter().cloned());

                    // Add transitive test dependents
                    for transitive_dep in &reverse_deps.transitive_dependents {
                        if self.is_test_module(transitive_dep) {
                            let test_nodeids =
                                self.get_nodeids_for_module(transitive_dep, tests_index);
                            selected_nodeids.extend(test_nodeids.iter().cloned());
                        }
                    }

                    if !reverse_deps.test_dependents.is_empty()
                        || reverse_deps
                            .transitive_dependents
                            .iter()
                            .any(|m| self.is_test_module(m))
                    {
                        let impacted_modules = reverse_deps
                            .test_dependents
                            .iter()
                            .chain(
                                reverse_deps
                                    .transitive_dependents
                                    .iter()
                                    .filter(|m| self.is_test_module(m)),
                            )
                            .cloned()
                            .collect();

                        selection_reasons.push(SelectionReason::SourceFileChanged {
                            path: file_path.clone(),
                            impacted_modules,
                        });
                    }
                }
            }
        }

        // Check for dynamic import safety valves
        if self.config.enable_dynamic_import_safety {
            for dynamic_import in &imports_graph.dynamic_imports {
                // Check if any changed modules could affect dynamic imports
                for file_path in changed_files {
                    if let Some(module_name) = changed_modules.get(file_path) {
                        if self.could_affect_dynamic_import(module_name, dynamic_import) {
                            // Broaden to all tests for safety
                            should_broaden = true;
                            broaden_reason = Some(format!(
                                "Dynamic import detected in {} - broadening for safety",
                                dynamic_import.from_module
                            ));
                            selection_reasons.push(SelectionReason::DynamicImportDetected {
                                from_module: dynamic_import.from_module.clone(),
                                reason: dynamic_import.reason.clone(),
                            });
                            break;
                        }
                    }
                }
                if should_broaden {
                    break;
                }
            }
        }

        // Apply threshold broadening rule
        let selection_percentage = selected_nodeids.len() as f64 / tests_index.tests.len() as f64;
        if selection_percentage > self.config.broaden_threshold {
            should_broaden = true;
            broaden_reason = Some(format!(
                "Selection threshold exceeded: {:.1}% > {:.1}% - running all tests",
                selection_percentage * 100.0,
                self.config.broaden_threshold * 100.0
            ));
            selection_reasons.push(SelectionReason::ThresholdExceeded {
                percentage: selection_percentage,
                threshold: self.config.broaden_threshold,
            });
        }

        // If broadening, select all tests
        let final_nodeids = if should_broaden {
            tests_index.tests.iter().map(|t| t.nodeid.clone()).collect()
        } else {
            selected_nodeids.into_iter().collect()
        };

        Ok(TestSelection {
            selected_nodeids: final_nodeids,
            total_tests: tests_index.tests.len(),
            selection_reasons,
            should_broaden,
            broaden_reason,
        })
    }

    /// Plan for full test run (--all flag)
    pub fn plan_full_run(&self, tests_index: &TestsIndex) -> TestSelection {
        TestSelection {
            selected_nodeids: tests_index.tests.iter().map(|t| t.nodeid.clone()).collect(),
            total_tests: tests_index.tests.len(),
            selection_reasons: vec![SelectionReason::FullRun {
                reason: "Full test run requested".to_string(),
            }],
            should_broaden: false,
            broaden_reason: None,
        }
    }

    /// Check if a file is a test file
    fn is_test_file(&self, file_path: &str) -> bool {
        let path = Path::new(file_path);
        if let Some(filename) = path.file_name() {
            let filename_str = filename.to_string_lossy();
            filename_str.starts_with("test_") || filename_str.ends_with("_test.py")
        } else {
            false
        }
    }

    /// Check if a module is a test module
    fn is_test_module(&self, module_name: &str) -> bool {
        module_name.starts_with("test_")
            || module_name.ends_with("_test")
            || module_name.contains(".test_")
            || module_name.contains(".tests.")
            || module_name.starts_with("tests.")
    }

    /// Get nodeids for a specific test file
    fn get_nodeids_for_test_file(&self, file_path: &str, tests_index: &TestsIndex) -> Vec<String> {
        tests_index
            .tests
            .iter()
            .filter(|test| test.path == file_path)
            .map(|test| test.nodeid.clone())
            .collect()
    }

    /// Get nodeids for tests under a conftest.py scope
    fn get_nodeids_for_conftest_scope(
        &self,
        conftest_path: &str,
        tests_index: &TestsIndex,
    ) -> Vec<String> {
        let conftest_dir = Path::new(conftest_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        tests_index
            .tests
            .iter()
            .filter(|test| test.path.starts_with(&conftest_dir))
            .map(|test| test.nodeid.clone())
            .collect()
    }

    /// Get conftest scope description
    fn get_conftest_scope(&self, conftest_path: &str) -> String {
        Path::new(conftest_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "root".to_string())
    }

    /// Get nodeids for a specific module
    fn get_nodeids_for_module(&self, module_name: &str, tests_index: &TestsIndex) -> Vec<String> {
        tests_index
            .tests
            .iter()
            .filter(|test| test.module == module_name)
            .map(|test| test.nodeid.clone())
            .collect()
    }

    /// Resolve changed file paths to module names
    fn resolve_changed_modules(
        &self,
        changed_files: &[String],
        module_map: &ModuleMap,
    ) -> Result<HashMap<String, String>> {
        let mut changed_modules = HashMap::new();

        for file_path in changed_files {
            // Try exact match first
            if let Some(module_info) = module_map.modules.get(file_path) {
                changed_modules.insert(file_path.clone(), module_info.module_name.clone());
                continue;
            }

            // Try with normalized path separators
            let normalized_path = file_path.replace('\\', "/");
            if let Some(module_info) = module_map.modules.get(&normalized_path) {
                changed_modules.insert(file_path.clone(), module_info.module_name.clone());
                continue;
            }

            // Try finding by relative path
            for (path, module_info) in &module_map.modules {
                if path == file_path || module_info.relative_path == *file_path {
                    changed_modules.insert(file_path.clone(), module_info.module_name.clone());
                    break;
                }
            }
        }

        Ok(changed_modules)
    }

    /// Check if a changed module could affect a dynamic import
    fn could_affect_dynamic_import(
        &self,
        changed_module: &str,
        dynamic_import: &crate::import_graph::DynamicImport,
    ) -> bool {
        // Conservative approach: any change could potentially affect dynamic imports
        // unless we have a static argument that we can check

        if let Some(argument) = &dynamic_import.argument {
            // If the dynamic import has a static argument, check if it matches
            argument == changed_module || argument.starts_with(&format!("{}.", changed_module))
        } else {
            // No static argument - be conservative and assume it could be affected
            true
        }
    }

    /// Format selection for explain mode
    pub fn format_explain(&self, selection: &TestSelection) -> String {
        let mut output = Vec::new();

        output.push("Test Selection Plan:".to_string());
        output.push(format!(
            "  Selected: {} of {} tests",
            selection.selected_nodeids.len(),
            selection.total_tests
        ));

        if selection.should_broaden {
            output.push("  Broadened: Yes".to_string());
            if let Some(reason) = &selection.broaden_reason {
                output.push(format!("  Reason: {}", reason));
            }
        } else {
            output.push("  Broadened: No".to_string());
        }

        output.push(String::new());
        output.push("Selection Reasons:".to_string());

        for (i, reason) in selection.selection_reasons.iter().enumerate() {
            match reason {
                SelectionReason::TestFileChanged { path } => {
                    output.push(format!("  {}. Test file changed: {}", i + 1, path));
                }
                SelectionReason::SourceFileChanged {
                    path,
                    impacted_modules,
                } => {
                    output.push(format!("  {}. Source file changed: {}", i + 1, path));
                    output.push(format!(
                        "     Impacted modules: {}",
                        impacted_modules.join(", ")
                    ));
                }
                SelectionReason::ConftestChanged { path, scope } => {
                    output.push(format!(
                        "  {}. conftest.py changed: {} (scope: {})",
                        i + 1,
                        path,
                        scope
                    ));
                }
                SelectionReason::PluginChanged { plugin } => {
                    output.push(format!("  {}. Plugin changed: {}", i + 1, plugin));
                }
                SelectionReason::DynamicImportDetected {
                    from_module,
                    reason,
                } => {
                    output.push(format!(
                        "  {}. Dynamic import detected in {}: {}",
                        i + 1,
                        from_module,
                        reason
                    ));
                }
                SelectionReason::ThresholdExceeded {
                    percentage,
                    threshold,
                } => {
                    output.push(format!(
                        "  {}. Threshold exceeded: {:.1}% > {:.1}%",
                        i + 1,
                        percentage * 100.0,
                        threshold * 100.0
                    ));
                }
                SelectionReason::FullRun { reason } => {
                    output.push(format!("  {}. Full run: {}", i + 1, reason));
                }
            }
        }

        if !selection.selected_nodeids.is_empty() && selection.selected_nodeids.len() <= 20 {
            output.push(String::new());
            output.push("Selected Tests:".to_string());
            for nodeid in &selection.selected_nodeids {
                output.push(format!("  - {}", nodeid));
            }
        } else if !selection.selected_nodeids.is_empty() {
            output.push(String::new());
            output.push(format!(
                "Selected Tests: {} tests (too many to list)",
                selection.selected_nodeids.len()
            ));
        }

        output.join("\n")
    }
}
