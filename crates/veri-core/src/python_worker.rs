//! Python worker integration for pytest compatibility
//!
//! This module provides the interface for communicating with the Python worker
//! that handles pytest collection and execution.

use crate::diagnostics::{DiagnosticReporter, VeriDiagnostic};
use crate::python_launcher::{PythonLaunchContext, PythonLauncher, PythonRuntime};
use anyhow::{anyhow, Context, Result};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Output};

pub use crate::schemas::{
    CollectionError, MarkerInfo, MarkersIndex, ParametrizeInfo, TestNode, TestsIndex,
};

/// Python worker client for executing pytest operations
pub struct PythonWorker {
    work_dir: PathBuf,
    cache_dir: PathBuf,
    py_worker_path: Option<PathBuf>,
    python_paths: Vec<PathBuf>,
    extra_env: Vec<(OsString, OsString)>,
    launcher: PythonLauncher,
}

impl PythonWorker {
    /// Create a new Python worker instance
    pub fn new(work_dir: impl Into<PathBuf>, cache_dir: impl Into<PathBuf>) -> Self {
        let work_dir = work_dir.into();
        let cache_dir = cache_dir.into();

        let py_worker_path = crate::paths::find_py_worker_path(&work_dir);
        let mut python_paths = Vec::new();
        if let Some(path) = &py_worker_path {
            python_paths.push(path.clone());
        }

        Self {
            work_dir,
            cache_dir,
            py_worker_path,
            python_paths,
            extra_env: Vec::new(),
            launcher: PythonLauncher::with_defaults(),
        }
    }

    /// Override the launcher (useful for tests or alternate environments).
    pub fn with_launcher(mut self, launcher: PythonLauncher) -> Self {
        self.launcher = launcher;
        self
    }

    /// Override the PYTHONPATH entries.
    pub fn with_python_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.python_paths = paths;
        self
    }

    /// Override environment variables injected into spawned processes.
    pub fn with_extra_env(mut self, env: Vec<(OsString, OsString)>) -> Self {
        self.extra_env = env;
        self
    }

    /// Override the detected py_worker project path.
    pub fn with_py_worker_path(mut self, py_worker_path: Option<PathBuf>) -> Self {
        self.py_worker_path = py_worker_path;
        if let Some(path) = &self.py_worker_path {
            if !self.python_paths.iter().any(|p| p == path) {
                self.python_paths.insert(0, path.clone());
            }
        }
        self
    }

    /// Construct a worker using a shared Python runtime definition.
    pub fn from_runtime(
        work_dir: impl Into<PathBuf>,
        cache_dir: impl Into<PathBuf>,
        runtime: &PythonRuntime,
    ) -> Self {
        Self::new(work_dir, cache_dir)
            .with_launcher(runtime.launcher.clone())
            .with_python_paths(runtime.python_paths.clone())
            .with_extra_env(runtime.extra_env.clone())
            .with_py_worker_path(runtime.py_worker_path.clone())
    }

    fn launch_context(&self) -> PythonLaunchContext<'_> {
        PythonLaunchContext::new(
            &self.work_dir,
            &self.cache_dir,
            &self.python_paths,
            self.py_worker_path.as_deref(),
            &self.extra_env,
        )
    }

    /// Collect tests using pytest and generate indexes
    pub fn collect_tests(&self, paths: &[String], ignores: &[String]) -> Result<TestsIndex> {
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

        // Add ignores
        for ig in ignores {
            args.push("--ignore".to_string());
            args.push(ig.clone());
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
        // Use absolute path to avoid cwd-related issues in the worker
        let module_map_path = std::fs::canonicalize(&module_map_path).unwrap_or(module_map_path);

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

        // Clean up temporary file - log warning if this fails
        std::fs::remove_file(&module_map_path).unwrap_or_else(|e| {
            log::warn!(
                "Failed to remove temp file {}: {}",
                module_map_path.display(),
                e
            );
        });

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
        let ctx = self.launch_context();
        let script = r#"
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
#,
        "#;
        let args = vec!["-c".to_string(), script.to_string()];
        let output = self
            .launcher
            .run(&ctx, &args)
            .with_context(|| "Failed to execute python command for pytest plugin discovery")?;

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

    /// Run a Python command with the configured launcher.
    fn run_python_command(&self, args: &[String]) -> Result<Output> {
        log::debug!(
            "run_python_command cwd={} args={:?}",
            self.work_dir.display(),
            args
        );
        let ctx = self.launch_context();
        self.launcher
            .run(&ctx, args)
            .with_context(|| format!("Failed to execute python command for args {:?}", args))
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
    pub ignore: Vec<String>,
    pub coverage: bool,
    pub coverage_xml: bool,
    pub coverage_html: bool,
    pub coverage_source_dirs: Vec<String>,
    pub coverage_omit: Vec<String>,
}

// (No direct single-run helper; all execution uses the worker pool.)
