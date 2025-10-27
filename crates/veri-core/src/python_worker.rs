//! Python worker integration for pytest compatibility
//!
//! This module provides the interface for communicating with the Python worker
//! that handles pytest collection and execution.

use crate::diagnostics::{DiagnosticReporter, VeriDiagnostic};
use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;
use std::process::{Command, Output};

pub use crate::schemas::{
    CollectionError, MarkerInfo, MarkersIndex, ParametrizeInfo, TestNode, TestsIndex,
};

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
        // Run Python to get installed pytest plugins (prefer python3, then python)
        let python = self.get_python_executable()?;
        let output = Command::new(&python)
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
            .with_context(|| format!("Failed to run Python to get pytest plugins using '{}')", python))?;

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
        log::debug!(
            "run_python_command cwd={} args={:?}",
            self.work_dir.display(),
            args
        );
        // Prefer running via `uv run -m veri_worker` if uv is available; otherwise
        // fall back to executing the cached veri_worker.py with the system Python.
        let uv_available = Command::new("uv").arg("--version").output().is_ok();

        if uv_available {
            // Prefer using the target project's environment so its dependencies resolve.
            // Make veri_worker importable by adding py_worker/ to PYTHONPATH.
            let py_worker_path = self.find_py_worker_path();
            let mut cmd = Command::new("uv");
            cmd.arg("run");
            cmd.arg("--project");
            cmd.arg(&self.work_dir);
            if let Some(py_worker) = &py_worker_path {
                // Ensure the worker module is importable within the project's env
                if let Ok(existing) = std::env::var("PYTHONPATH") {
                    let mut val = py_worker.to_string_lossy().to_string();
                    val.push(std::path::MAIN_SEPARATOR);
                    val.push_str(&existing);
                    cmd.env("PYTHONPATH", val);
                } else {
                    cmd.env("PYTHONPATH", py_worker);
                }
            }

            cmd.args(args);
            cmd.current_dir(&self.work_dir);
            // Try uv path first
            match cmd.output().context("Failed to execute 'uv run' command") {
                Ok(out) => {
                    log::debug!(
                        "uv run status={} stdout_len={} stderr_len={}",
                        out.status,
                        out.stdout.len(),
                        out.stderr.len()
                    );
                    // If uv executed but returned failure, attempt python fallback
                    if !out.status.success() {
                        log::debug!("uv run failed; attempting python fallback");
                        let python = self.get_python_executable()?;
                        let worker_script = self.ensure_worker_script()?;
                        let adjusted = Self::adjust_args_for_script(args);
                        let mut py = Command::new(&python);
                        let fb = py
                            .arg(worker_script)
                            .args(&adjusted)
                            .current_dir(&self.work_dir)
                            .output()
                            .with_context(|| {
                                format!(
                                    "Failed to execute Python worker via '{}' after uv failure",
                                    python
                                )
                            })?;
                        return Ok(fb);
                    }
                    return Ok(out);
                }
                Err(e) => {
                    // If uv failed to spawn at all, fall back immediately
                    log::debug!("uv run spawn error: {} — attempting python fallback", e);
                    let python = self.get_python_executable()?;
                    let worker_script = self.ensure_worker_script()?;
                    let adjusted = Self::adjust_args_for_script(args);
                    let mut py = Command::new(&python);
                    return py
                        .arg(worker_script)
                        .args(&adjusted)
                        .current_dir(&self.work_dir)
                        .output()
                        .with_context(|| {
                            format!(
                                "Failed to execute Python worker via '{}' after uv error: {}",
                                python, e
                            )
                        });
                }
            }
        }

        // Fallback: run the worker script directly with python3/python
        let python = self.get_python_executable()?;
        let worker_script = self.ensure_worker_script()?;

        let adjusted = Self::adjust_args_for_script(args);

        let mut cmd = Command::new(&python);
        cmd.arg(worker_script)
            .args(&adjusted)
            .current_dir(&self.work_dir);
        log::debug!("python fallback using {} args={:?}", python, adjusted);
        cmd.output().with_context(|| {
            format!(
                "Failed to execute Python worker via '{}' (uv not found)",
                python
            )
        })
    }

    fn adjust_args_for_script(args: &[String]) -> Vec<String> {
        // The incoming args are typically ["-m", "veri_worker", <rest>]. When
        // running the script file directly, drop the "-m veri_worker" prefix.
        let mut adjusted: Vec<String> = Vec::new();
        let mut iter = args.iter();
        if let (Some(first), Some(second)) = (iter.next(), iter.next()) {
            if first == "-m" && second == "veri_worker" {
                adjusted.extend(iter.cloned());
            } else {
                adjusted = args.to_vec();
            }
        } else {
            adjusted = args.to_vec();
        }
        adjusted
    }

    /// Find the py_worker directory path
    fn find_py_worker_path(&self) -> Option<PathBuf> {
        crate::paths::find_py_worker_path(&self.work_dir)
    }

    /// Ensure the worker script is available in the cache dir and return its path
    fn ensure_worker_script(&self) -> Result<PathBuf> {
        let dest = self.cache_dir.join("veri_worker.py");
        if dest.exists() {
            return Ok(dest);
        }

        // Try to locate the source worker script in common locations
        let mut candidates: Vec<PathBuf> = Vec::new();
        // 1) work_dir/py_worker/veri_worker.py
        candidates.push(self.work_dir.join("py_worker").join("veri_worker.py"));
        // 2) repo-relative to this crate (../../py_worker/veri_worker.py)
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        if let Some(p) = manifest_dir.parent().and_then(|p| p.parent()) {
            candidates.push(p.join("py_worker").join("veri_worker.py"));
        }
        // 3) work_dir/veri_worker.py
        candidates.push(self.work_dir.join("veri_worker.py"));

        let src = candidates
            .into_iter()
            .find(|p| p.exists())
            .ok_or_else(|| anyhow!("Could not find veri_worker.py; expected under py_worker/"))?;

        std::fs::create_dir_all(&self.cache_dir)
            .context("Failed to create cache directory for worker script")?;
        std::fs::copy(&src, &dest).with_context(|| {
            format!(
                "Failed to copy worker script from {} to {}",
                src.display(),
                dest.display()
            )
        })?;
        Ok(dest)
    }

    /// Find a usable Python executable (python3, then python, then py on Windows)
    fn get_python_executable(&self) -> Result<String> {
        let candidates = ["python3", "python", "py"];
        for c in &candidates {
            if let Ok(out) = Command::new(c).arg("--version").output() {
                if out.status.success() {
                    return Ok(c.to_string());
                }
            }
        }
        Err(anyhow!(
            "Could not find a Python interpreter (tried python3, python, py)"
        ))
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
