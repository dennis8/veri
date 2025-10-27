use anyhow::Result;
use veri_core::python_worker::TestsIndex;
use veri_core::import_graph::{ImportsGraph, ReverseDepsGraph, ModuleMap};
use veri_core::python_launcher::PythonRuntime;
use std::path::Path;

use crate::cli::{Cli, ExitCode};
use super::selection::SelectionCriteria;
use super::services::OrchestratorServices;

/// Result of test selection operation
#[derive(Debug, Clone)]
pub struct SelectionOutcome {
    /// Test node IDs selected to run
    pub nodeids: Vec<String>,
    /// Selection statistics
    pub stats: SelectionStats,
    /// If Some, we should exit early with this code (e.g., no tests found)
    pub early_exit: Option<ExitCode>,
}

/// Statistics about test selection
#[derive(Debug, Clone)]
pub struct SelectionStats {
    pub total_tests: usize,
    pub selected_tests: usize,
    pub fallback_triggered: bool,
}

/// Trait for orchestrating test selection
pub trait SelectionOrchestrationService: Send + Sync {
    fn determine_tests_to_run(
        &self,
        tests_index: &TestsIndex,
        cli: &Cli,
        services: &OrchestratorServices,
        graphs: Option<(&ImportsGraph, &ReverseDepsGraph, &ModuleMap)>,
    ) -> Result<SelectionOutcome>;
}

/// Default implementation for test selection
pub struct DefaultSelectionOrchestrator {
    work_dir: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
    python_runtime: PythonRuntime,
}

impl DefaultSelectionOrchestrator {
    pub fn new(
        work_dir: &Path,
        cache_dir: &Path,
        python_runtime: PythonRuntime,
    ) -> Self {
        Self {
            work_dir: work_dir.to_path_buf(),
            cache_dir: cache_dir.to_path_buf(),
            python_runtime,
        }
    }
}

impl SelectionOrchestrationService for DefaultSelectionOrchestrator {
    fn determine_tests_to_run(
        &self,
        tests_index: &TestsIndex,
        cli: &Cli,
        services: &OrchestratorServices,
        graphs: Option<(&ImportsGraph, &ReverseDepsGraph, &ModuleMap)>,
    ) -> Result<SelectionOutcome> {
        // If --all flag is set, run all tests
        if cli.all {
            let total = tests_index.tests.len();
            return Ok(SelectionOutcome {
                nodeids: tests_index.tests.iter().map(|t| t.nodeid.clone()).collect(),
                stats: SelectionStats {
                    total_tests: total,
                    selected_tests: total,
                    fallback_triggered: false,
                },
                early_exit: None,
            });
        }

        // For impact analysis: we must have graphs for non-all selection
        let (imports_graph, revdeps_graph, module_map) = if let Some((ig, rg, mm)) = graphs {
            (ig, rg, mm)
        } else {
            // This shouldn't happen with correct orchestration
            return Err(anyhow::anyhow!(
                "Internal error: graphs should be provided for non-all selection"
            ));
        };

        // Get changed files for impact analysis
        let changed_files = services.vcs.get_changed_files()?;

        // Create selection criteria from CLI
        let criteria = SelectionCriteria {
            all: cli.all,
            last_failed: cli.last_failed,
            keyword: cli.keyword.clone(),
            marker: cli.marker.clone(),
            paths: cli.paths.clone(),
            changed_files,
        };

        // Use selection service to determine which tests to run
        let selection = services.selection.select_tests(
            tests_index,
            imports_graph,
            revdeps_graph,
            module_map,
            &criteria,
        )?;

        let mut nodeids_to_run = selection.selected_nodeids;

        // Fallback: if nothing selected but tests exist, run all tests
        let fallback_triggered = if nodeids_to_run.is_empty() && !tests_index.tests.is_empty() {
            println!("ℹ️  No impacted tests detected; defaulting to run all tests");
            nodeids_to_run = tests_index.tests.iter().map(|t| t.nodeid.clone()).collect();
            true
        } else {
            false
        };

        // Error if no tests at all
        if nodeids_to_run.is_empty() {
            return Ok(SelectionOutcome {
                nodeids: vec![],
                stats: SelectionStats {
                    total_tests: tests_index.tests.len(),
                    selected_tests: 0,
                    fallback_triggered,
                },
                early_exit: Some(ExitCode::UsageError),
            });
        }

        println!("🎯 Running {} selected tests", nodeids_to_run.len());

        Ok(SelectionOutcome {
            nodeids: nodeids_to_run.clone(),
            stats: SelectionStats {
                total_tests: tests_index.tests.len(),
                selected_tests: nodeids_to_run.len(),
                fallback_triggered,
            },
            early_exit: None,
        })
    }
}

#[cfg(test)]
mod testing {
    use super::*;
    use std::sync::Arc;
    use std::sync::Mutex;

    /// Mock selection orchestrator for testing
    pub struct MockSelectionOrchestrator {
        pub outcome: Arc<Mutex<SelectionOutcome>>,
    }

    impl MockSelectionOrchestrator {
        pub fn new(outcome: SelectionOutcome) -> Self {
            Self {
                outcome: Arc::new(Mutex::new(outcome)),
            }
        }

        pub fn with_selected_count(total: usize, selected: usize) -> Self {
            let nodeids = (0..selected).map(|i| format!("test_{}", i)).collect();
            Self::new(SelectionOutcome {
                nodeids,
                stats: SelectionStats {
                    total_tests: total,
                    selected_tests: selected,
                    fallback_triggered: false,
                },
                early_exit: None,
            })
        }
    }

    impl SelectionOrchestrationService for MockSelectionOrchestrator {
        fn determine_tests_to_run(
            &self,
            _tests_index: &TestsIndex,
            _cli: &Cli,
            _services: &OrchestratorServices,
            _graphs: Option<(&ImportsGraph, &ReverseDepsGraph, &ModuleMap)>,
        ) -> Result<SelectionOutcome> {
            Ok(self
                .outcome
                .lock()
                .map_err(|e| anyhow::anyhow!("Mock lock failed: {}", e))?
                .clone())
        }
    }
}
