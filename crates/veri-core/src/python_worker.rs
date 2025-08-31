//! Python worker integration for pytest compatibility
//!
//! This module provides the interface for communicating with the Python worker
//! that handles pytest collection and execution.

use crate::diagnostics::{DiagnosticReporter, VeriDiagnostic};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Output};

/// Test collection data from tests.index.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestsIndex {
    pub version: String,
    pub generated_at: String,
    pub python_version: String,
    pub pytest_version: String,
    pub tests: Vec<TestNode>,
    pub collection_errors: Vec<CollectionError>,
}

/// Individual test node information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestNode {
    pub nodeid: String,
    pub path: String,
    pub line: u32,
    pub function: String,
    pub class: Option<String>,
    pub module: String,
    pub markers: Vec<String>,
    pub fixtures: Vec<String>,
    pub parametrize: Option<ParametrizeInfo>,
}

/// Parametrization information for tests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParametrizeInfo {
    pub params: Vec<String>,
    pub ids: Vec<String>,
}

/// Collection error information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionError {
    pub path: String,
    pub line: Option<u32>,
    pub error_type: String,
    pub message: String,
}

/// Marker index data from markers.index.json
#[derive(Debug, Serialize, Deserialize)]
pub struct MarkersIndex {
    pub version: String,
    pub generated_at: String,
    pub markers: HashMap<String, MarkerInfo>,
    pub test_markers: HashMap<String, Vec<String>>,
}

/// Information about a specific marker
#[derive(Debug, Serialize, Deserialize)]
pub struct MarkerInfo {
    pub name: String,
    pub description: Option<String>,
    pub registered: bool,
    pub usage_count: u32,
    pub first_seen: String,
    pub common_args: Vec<String>,
}

/// Python worker client for executing pytest operations
pub struct PythonWorker {
    work_dir: PathBuf,
    cache_dir: PathBuf,
    python_executable: String,
}

impl PythonWorker {
    /// Create a new Python worker instance
    pub fn new(work_dir: impl Into<PathBuf>, cache_dir: impl Into<PathBuf>) -> Self {
        let work_dir = work_dir.into();
        let cache_dir = cache_dir.into();

        // Always use uv run for consistent dependency management
        let python_executable = "uv".to_string();

        Self {
            work_dir,
            cache_dir,
            python_executable,
        }
    }
    /// Set the Python executable to use
    pub fn with_python_executable(mut self, executable: String) -> Self {
        self.python_executable = executable;
        self
    }

    /// Collect tests using pytest and generate indexes
    pub fn collect_tests(&self, paths: &[String]) -> Result<TestsIndex> {
        // Ensure cache directory exists
        std::fs::create_dir_all(&self.cache_dir).context("Failed to create cache directory")?;

        // Build command arguments
        let mut args = vec![
            "-m".to_string(),
            "veri_worker".to_string(),
            "collect".to_string(),
            "--work-dir".to_string(),
            self.work_dir.to_string_lossy().to_string(),
            "--cache-dir".to_string(),
            self.cache_dir.to_string_lossy().to_string(),
        ];

        if !paths.is_empty() {
            args.push("--paths".to_string());
            args.extend(paths.iter().cloned());
        }

        // Execute Python worker
        let output = self
            .run_python_command(&args)
            .context("Failed to run Python worker for test collection")?;

        // Check for fatal errors (exit codes other than 0 and 2)
        let exit_code = output.status.code().unwrap_or(-1);
        if !output.status.success() && exit_code != 2 {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Test collection failed: {}", stderr));
        }

        // Load the generated tests.index.json
        let tests_index_path = self.cache_dir.join("tests.index.json");
        let tests_data = std::fs::read_to_string(&tests_index_path)
            .context("Failed to read tests.index.json")?;

        let tests_index: TestsIndex =
            serde_json::from_str(&tests_data).context("Failed to parse tests.index.json")?;

        Ok(tests_index)
    }

    /// Load markers index from cache
    pub fn load_markers_index(&self) -> Result<MarkersIndex> {
        let markers_index_path = self.cache_dir.join("markers.index.json");
        let markers_data = std::fs::read_to_string(&markers_index_path)
            .context("Failed to read markers.index.json")?;

        let markers_index: MarkersIndex =
            serde_json::from_str(&markers_data).context("Failed to parse markers.index.json")?;

        Ok(markers_index)
    }

    /// Execute specific tests by nodeid
    pub fn run_tests(&self, nodeids: &[String], options: &TestRunOptions) -> Result<i32> {
        let mut args = vec![
            "-m".to_string(),
            "veri_worker".to_string(),
            "run".to_string(),
            "--work-dir".to_string(),
            self.work_dir.to_string_lossy().to_string(),
        ];

        // Add nodeids
        if !nodeids.is_empty() {
            args.push("--nodeids".to_string());
            args.extend(nodeids.iter().cloned());
        }

        // Add options
        if options.verbose {
            args.push("--verbose".to_string());
        }
        if options.quiet {
            args.push("--quiet".to_string());
        }
        if options.no_capture {
            args.push("--no-capture".to_string());
        }
        if options.exitfirst {
            args.push("--exitfirst".to_string());
        }
        if let Some(maxfail) = options.maxfail {
            args.push("--maxfail".to_string());
            args.push(maxfail.to_string());
        }
        if let Some(junit_xml) = &options.junit_xml {
            args.push("--junit-xml".to_string());
            args.push(junit_xml.to_string_lossy().to_string());
        }
        if let Some(workers) = &options.workers {
            args.push("--workers".to_string());
            args.push(workers.clone());
        }

        // Add coverage options
        if options.coverage {
            args.push("--coverage".to_string());
        }
        if options.coverage_xml {
            args.push("--coverage-xml".to_string());
        }
        if options.coverage_html {
            args.push("--coverage-html".to_string());
        }
        if !options.coverage_source_dirs.is_empty() {
            args.push("--coverage-source-dirs".to_string());
            args.extend(options.coverage_source_dirs.iter().cloned());
        }
        if !options.coverage_omit.is_empty() {
            args.push("--coverage-omit".to_string());
            args.extend(options.coverage_omit.iter().cloned());
        }

        // Execute Python worker
        let output = self
            .run_python_command(&args)
            .context("Failed to run Python worker for test execution")?;

        Ok(output.status.code().unwrap_or(3))
    }

    /// Hand off completely to pytest (--engine pytest mode)
    pub fn run_pytest_engine(&self, pytest_args: &[String]) -> Result<i32> {
        let mut args = vec![
            "-m".to_string(),
            "veri_worker".to_string(),
            "pytest-engine".to_string(),
            "--work-dir".to_string(),
            self.work_dir.to_string_lossy().to_string(),
        ];

        if !pytest_args.is_empty() {
            args.push("--pytest-args".to_string());
            args.extend(pytest_args.iter().cloned());
        }

        // Execute Python worker
        let output = self
            .run_python_command(&args)
            .context("Failed to run Python worker in pytest engine mode")?;

        Ok(output.status.code().unwrap_or(3))
    }

    /// Parse imports from Python files using AST analysis
    pub fn parse_imports(
        &self,
        module_map: &crate::import_graph::ModuleMap,
    ) -> Result<crate::import_graph::ImportsGraph> {
        // Save module map to a temporary file for the Python worker
        let module_map_path = self.cache_dir.join("temp_module_map.json");
        let module_map_json = serde_json::to_string_pretty(module_map)?;
        std::fs::write(&module_map_path, module_map_json)?;

        // Build command arguments
        let args = vec![
            "-m".to_string(),
            "veri_worker".to_string(),
            "parse-imports".to_string(),
            "--work-dir".to_string(),
            self.work_dir.to_string_lossy().to_string(),
            "--cache-dir".to_string(),
            self.cache_dir.to_string_lossy().to_string(),
            "--module-map".to_string(),
            module_map_path.to_string_lossy().to_string(),
        ];

        // Execute Python worker
        let output = self
            .run_python_command(&args)
            .context("Failed to run Python worker for import parsing")?;

        // Clean up temporary file
        let _ = std::fs::remove_file(module_map_path);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Import parsing failed: {}", stderr));
        }

        // Load the generated imports.graph.json
        let imports_graph_path = self.cache_dir.join("imports.graph.json");
        let imports_data = std::fs::read_to_string(&imports_graph_path)
            .context("Failed to read imports.graph.json")?;

        let imports_graph: crate::import_graph::ImportsGraph =
            serde_json::from_str(&imports_data).context("Failed to parse imports.graph.json")?;

        Ok(imports_graph)
    }

    /// Get list of installed pytest plugins
    pub fn get_pytest_plugins(&self) -> Result<Vec<String>> {
        // Run Python to get installed pytest plugins
        let output = Command::new("python")
            .args([
                "-c",
                r#"
import pkg_resources
import json
import sys

try:
    # Get all installed packages
    installed_packages = [d.project_name for d in pkg_resources.working_set]
    
    # Filter for pytest plugins (packages that start with 'pytest-' or are pytest itself)
    plugins = [pkg for pkg in installed_packages if pkg.startswith('pytest') or 'pytest' in pkg.lower()]
    
    # Also check for setuptools entry points for pytest plugins
    try:
        for entry_point in pkg_resources.iter_entry_points('pytest11'):
            plugin_name = entry_point.dist.project_name
            if plugin_name not in plugins:
                plugins.append(plugin_name)
    except:
        pass
    
    # Get version information for each plugin
    plugin_info = []
    for plugin in plugins:
        try:
            dist = pkg_resources.get_distribution(plugin)
            plugin_info.append(f"{dist.project_name}=={dist.version}")
        except:
            plugin_info.append(plugin)
    
    print(json.dumps(plugin_info))
except Exception as e:
    print(json.dumps([]), file=sys.stderr)
    print(f"Error getting plugins: {e}", file=sys.stderr)
"#,
            ])
            .current_dir(&self.work_dir)
            .output()
            .context("Failed to run Python to get pytest plugins")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Failed to get pytest plugins: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let plugins: Vec<String> =
            serde_json::from_str(&stdout).context("Failed to parse pytest plugins JSON")?;

        Ok(plugins)
    }

    /// Check if tests.index.json exists and is recent
    pub fn has_valid_cache(&self) -> bool {
        let tests_index_path = self.cache_dir.join("tests.index.json");
        let markers_index_path = self.cache_dir.join("markers.index.json");

        tests_index_path.exists() && markers_index_path.exists()
    }

    /// Get the path to the tests index file
    pub fn tests_index_path(&self) -> PathBuf {
        self.cache_dir.join("tests.index.json")
    }

    /// Get the path to the markers index file  
    pub fn markers_index_path(&self) -> PathBuf {
        self.cache_dir.join("markers.index.json")
    }

    /// Run a Python command with the configured executable
    fn run_python_command(&self, args: &[String]) -> Result<Output> {
        // Find the py_worker directory to use as the project root for uv
        let py_worker_path = self.find_py_worker_path();

        let mut cmd = Command::new(&self.python_executable);

        // Add uv run arguments with project specification
        cmd.arg("run");
        if let Some(py_worker) = &py_worker_path {
            cmd.arg("--project");
            cmd.arg(py_worker);
        }

        // Add the python module arguments
        cmd.args(args);
        cmd.current_dir(&self.work_dir);

        cmd.output().context("Failed to execute uv run command")
    }

    /// Find the py_worker directory path
    fn find_py_worker_path(&self) -> Option<PathBuf> {
        // Look for py_worker directory
        if let Ok(current_exe) = std::env::current_exe() {
            if let Some(parent) = current_exe.parent() {
                let mut potential_root = parent.to_path_buf();
                if potential_root.ends_with("debug") {
                    potential_root.pop(); // Remove "debug"
                    potential_root.pop(); // Remove "target"
                }

                let potential_py_worker = potential_root.join("py_worker");
                if potential_py_worker.exists() {
                    return Some(potential_py_worker);
                }
            }
        }

        // If not found relative to exe, try relative to work_dir
        let potential_py_worker = self.work_dir.join("py_worker");
        if potential_py_worker.exists() {
            return Some(potential_py_worker);
        }

        // Try going up from work_dir to find project root
        let mut current = self.work_dir.clone();
        for _ in 0..5 {
            // Try up to 5 levels up
            let potential_py_worker = current.join("py_worker");
            if potential_py_worker.exists() {
                return Some(potential_py_worker);
            }
            if !current.pop() {
                break;
            }
        }

        None
    }

    /// Check for Python environment issues and generate diagnostics
    pub fn check_environment(&self, diagnostics: &mut DiagnosticReporter) -> Result<()> {
        // Check if Python is available
        let python_result = Command::new("python").arg("--version").output();

        match python_result {
            Ok(output) => {
                if !output.status.success() {
                    diagnostics.add(VeriDiagnostic::PythonEnvironmentIssue {
                        issue_type: crate::diagnostics::PythonIssueType::NotFound,
                        current_python: "python not found".to_string(),
                        suggestions: vec![
                            "Install Python 3.8 or higher".to_string(),
                            "Ensure Python is in your PATH".to_string(),
                            "Try 'python3' if 'python' doesn't work".to_string(),
                        ],
                    });
                } else {
                    let version_output = String::from_utf8_lossy(&output.stdout);

                    // Check for pytest availability
                    let pytest_result = Command::new("python")
                        .args(["-m", "pytest", "--version"])
                        .output();

                    if let Ok(pytest_output) = pytest_result {
                        if !pytest_output.status.success() {
                            diagnostics.add(VeriDiagnostic::PythonEnvironmentIssue {
                                issue_type:
                                    crate::diagnostics::PythonIssueType::MissingDependencies(vec![
                                        "pytest".to_string(),
                                    ]),
                                current_python: version_output.trim().to_string(),
                                suggestions: vec![
                                    "Install pytest: pip install pytest".to_string(),
                                    "Or: uv add pytest".to_string(),
                                    "Check virtual environment activation".to_string(),
                                ],
                            });
                        }
                    }
                }
            }
            Err(_) => {
                diagnostics.add(VeriDiagnostic::PythonEnvironmentIssue {
                    issue_type: crate::diagnostics::PythonIssueType::NotFound,
                    current_python: "python not accessible".to_string(),
                    suggestions: vec![
                        "Install Python 3.8 or higher".to_string(),
                        "Ensure Python is in your PATH".to_string(),
                        "Activate virtual environment if using one".to_string(),
                    ],
                });
            }
        }

        Ok(())
    }

    /// Check tests index for collection errors and generate diagnostics
    pub fn check_collection_errors(
        &self,
        tests_index: &TestsIndex,
        diagnostics: &mut DiagnosticReporter,
    ) {
        if !tests_index.collection_errors.is_empty() {
            let syntax_errors = tests_index
                .collection_errors
                .iter()
                .filter(|e| e.error_type.contains("Syntax"))
                .map(|e| e.message.clone())
                .collect();

            let import_errors = tests_index
                .collection_errors
                .iter()
                .filter(|e| e.error_type.contains("Import") || e.error_type.contains("Module"))
                .map(|e| e.path.clone())
                .collect();

            diagnostics.add(VeriDiagnostic::ImportGraphBuildFailed {
                error_count: tests_index.collection_errors.len(),
                syntax_errors,
                missing_files: import_errors,
            });
        }
    }
}

/// Options for test execution
#[derive(Debug, Default, Clone)]
pub struct TestRunOptions {
    pub verbose: bool,
    pub quiet: bool,
    pub no_capture: bool,
    pub exitfirst: bool,
    pub maxfail: Option<u32>,
    pub junit_xml: Option<PathBuf>,
    pub workers: Option<String>,
    pub coverage: bool,
    pub coverage_xml: bool,
    pub coverage_html: bool,
    pub coverage_source_dirs: Vec<String>,
    pub coverage_omit: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn collects_and_runs_tests() {
        let work_dir = TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let cache_dir = work_dir.path().join(".veri").join("cache");
        std::fs::create_dir_all(&cache_dir).unwrap();

        fs::write(
            work_dir.path().join("test_sample.py"),
            "def test_ok():\n    assert 1 + 1 == 2\n",
        )
        .unwrap();

        let worker = PythonWorker::new(work_dir.path(), &cache_dir);
        let index = worker.collect_tests(&[]).expect("collect tests");
        assert_eq!(index.tests.len(), 1);

        let exit_code = worker.run_pytest_engine(&[]).expect("run tests");
        assert_eq!(exit_code, 0);
    }
}
