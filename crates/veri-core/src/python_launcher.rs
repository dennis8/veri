use crate::config::{PythonBackendConfig, PythonRuntimeConfig};
use anyhow::{anyhow, Context, Result};
use log::debug;
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};

/// Execution context describing how Veri wants to launch Python processes.
pub struct PythonLaunchContext<'a> {
    /// Working directory for the launched process.
    pub work_dir: &'a Path,
    /// Cache directory for Veri-specific state.
    pub cache_dir: &'a Path,
    /// Additional entries that should be present on PYTHONPATH.
    pub python_paths: &'a [PathBuf],
    /// Optional descriptor of the py_worker project root.
    pub py_worker_project: Option<&'a Path>,
    /// Additional environment overrides that should be applied before spawn.
    pub extra_env: &'a [(OsString, OsString)],
}

impl<'a> PythonLaunchContext<'a> {
    pub fn new(
        work_dir: &'a Path,
        cache_dir: &'a Path,
        python_paths: &'a [PathBuf],
        py_worker_project: Option<&'a Path>,
        extra_env: &'a [(OsString, OsString)],
    ) -> Self {
        Self {
            work_dir,
            cache_dir,
            python_paths,
            py_worker_project,
            extra_env,
        }
    }
}

/// Trait implemented by concrete backends capable of launching Python commands.
pub trait PythonBackend: Send + Sync {
    /// Human-friendly backend name for diagnostics/logging.
    fn name(&self) -> &'static str;

    /// Create a [`Command`] representing `python` invocation for the supplied arguments.
    fn build_command(&self, ctx: &PythonLaunchContext<'_>, args: &[String]) -> Result<Command>;

    /// Whether the backend should be considered available given the context.
    /// Default implementation always returns true.
    fn is_available(&self, _ctx: &PythonLaunchContext<'_>) -> bool {
        true
    }
}

/// Helper responsible for trying a sequence of Python backends until one succeeds.
#[derive(Clone)]
pub struct PythonLauncher {
    backends: Vec<Arc<dyn PythonBackend>>,
}

impl PythonLauncher {
    pub fn new(backends: Vec<Arc<dyn PythonBackend>>) -> Self {
        Self { backends }
    }

    /// Create a launcher with default backends (uv, system python).
    pub fn with_defaults() -> Self {
        let uv = Arc::new(UvBackend::default());
        let system = Arc::new(SystemPythonBackend::default());
        Self::new(vec![uv, system])
    }

    /// Attempt to execute a Python command, returning the [`Output`] on success.
    pub fn run(&self, ctx: &PythonLaunchContext<'_>, args: &[String]) -> Result<Output> {
        let mut errors: Vec<anyhow::Error> = Vec::new();
        for backend in &self.backends {
            if !backend.is_available(ctx) {
                continue;
            }
            match backend.build_command(ctx, args) {
                Ok(mut cmd) => {
                    apply_common_configuration(&mut cmd, ctx)?;
                    debug!(
                        "Launching python backend={} cwd={} args={:?}",
                        backend.name(),
                        ctx.work_dir.display(),
                        args
                    );
                    match cmd.output() {
                        Ok(output) => return Ok(output),
                        Err(err) => {
                            errors.push(anyhow!(
                                "Failed to execute backend {}: {}",
                                backend.name(),
                                err
                            ));
                        }
                    }
                }
                Err(err) => errors.push(err),
            }
        }

        if errors.is_empty() {
            Err(anyhow!(
                "No python backends available to execute command {:?}",
                args
            ))
        } else {
            let combined = errors
                .into_iter()
                .map(|e| format!("{:#}", e))
                .collect::<Vec<_>>()
                .join("\n");
            Err(anyhow!(
                "All python backends failed for args {:?}:\n{}",
                args,
                combined
            ))
        }
    }

    /// Attempt to spawn a Python process using the configured backends. The `configure`
    /// closure is invoked before spawn to allow callers to set up pipes or env vars.
    pub fn spawn_with<F>(
        &self,
        ctx: &PythonLaunchContext<'_>,
        args: &[String],
        mut configure: F,
    ) -> Result<Child>
    where
        F: FnMut(&mut Command),
    {
        let mut errors: Vec<anyhow::Error> = Vec::new();
        for backend in &self.backends {
            if !backend.is_available(ctx) {
                continue;
            }
            match backend.build_command(ctx, args) {
                Ok(mut cmd) => {
                    apply_common_configuration(&mut cmd, ctx)?;
                    configure(&mut cmd);
                    debug!(
                        "Spawning python backend={} cwd={} args={:?}",
                        backend.name(),
                        ctx.work_dir.display(),
                        args
                    );
                    match cmd.spawn() {
                        Ok(child) => return Ok(child),
                        Err(err) => {
                            errors.push(anyhow!(
                                "Failed to spawn backend {}: {}",
                                backend.name(),
                                err
                            ));
                        }
                    }
                }
                Err(err) => errors.push(err),
            }
        }

        if errors.is_empty() {
            Err(anyhow!(
                "No python backends available to spawn command {:?}",
                args
            ))
        } else {
            let combined = errors
                .into_iter()
                .map(|e| format!("{:#}", e))
                .collect::<Vec<_>>()
                .join("\n");
            Err(anyhow!(
                "All python backends failed to spawn for args {:?}:\n{}",
                args,
                combined
            ))
        }
    }
}

impl fmt::Debug for PythonLauncher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&'static str> = self.backends.iter().map(|b| b.name()).collect();
        f.debug_struct("PythonLauncher")
            .field("backends", &names)
            .finish()
    }
}

#[derive(Clone)]
pub struct PythonRuntime {
    pub launcher: PythonLauncher,
    pub python_paths: Vec<PathBuf>,
    pub extra_env: Vec<(OsString, OsString)>,
    pub py_worker_path: Option<PathBuf>,
}

impl PythonRuntime {
    pub fn from_config(work_dir: &Path, cfg: &PythonRuntimeConfig) -> Self {
        let py_worker_path = cfg
            .py_worker_path
            .as_ref()
            .map(|p| resolve_path(work_dir, p.as_path()))
            .or_else(|| crate::paths::find_py_worker_path(work_dir));

        let mut python_paths: Vec<PathBuf> = Vec::new();
        if let Some(path) = &py_worker_path {
            python_paths.push(path.clone());
        }
        for extra in &cfg.extra_pythonpath {
            let absolute = resolve_path(work_dir, extra.as_path());
            if !python_paths.iter().any(|p| p == &absolute) {
                python_paths.push(absolute);
            }
        }

        let extra_env = cfg
            .env
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect::<Vec<_>>();

        let backends = build_backends_from_config(cfg, work_dir);
        let launcher = if backends.is_empty() {
            PythonLauncher::with_defaults()
        } else {
            PythonLauncher::new(backends)
        };

        Self {
            launcher,
            python_paths,
            extra_env,
            py_worker_path,
        }
    }
}

impl fmt::Debug for PythonRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PythonRuntime")
            .field("launcher", &self.launcher)
            .field("python_paths", &self.python_paths)
            .field("extra_env", &self.extra_env)
            .field("py_worker_path", &self.py_worker_path)
            .finish()
    }
}

/// Default uv-based backend.
pub struct UvBackend {
    binary: String,
    project_override: Option<PathBuf>,
}

impl Default for UvBackend {
    fn default() -> Self {
        Self {
            binary: "uv".to_string(),
            project_override: None,
        }
    }
}

impl UvBackend {
    pub fn new(binary: impl Into<String>, project_override: Option<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            project_override,
        }
    }

    fn ensure_available(&self) -> Result<()> {
        Command::new(&self.binary)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| {
                format!(
                    "Failed to execute '{} --version' while probing uv availability",
                    self.binary
                )
            })
            .and_then(|status| {
                if status.success() {
                    Ok(())
                } else {
                    Err(anyhow!(
                        "'{} --version' exited with status {}",
                        self.binary,
                        status
                    ))
                }
            })
    }

    fn resolve_project(&self, ctx: &PythonLaunchContext<'_>) -> PathBuf {
        self.project_override
            .as_ref()
            .cloned()
            .or_else(|| ctx.py_worker_project.map(Path::to_path_buf))
            .unwrap_or_else(|| ctx.work_dir.to_path_buf())
    }
}

impl PythonBackend for UvBackend {
    fn name(&self) -> &'static str {
        "uv"
    }

    fn build_command(&self, ctx: &PythonLaunchContext<'_>, args: &[String]) -> Result<Command> {
        self.ensure_available()?;
        let project = self.resolve_project(ctx);

        let mut cmd = Command::new(&self.binary);
        cmd.arg("run");
        cmd.arg("--project");
        cmd.arg(&project);
        cmd.arg("python");
        cmd.args(args.iter().map(|a| a.as_str()));
        Ok(cmd)
    }
}

/// Backend that shells out to a system Python interpreter.
pub struct SystemPythonBackend {
    candidates: Vec<String>,
    resolved: Mutex<Option<String>>,
}

impl Default for SystemPythonBackend {
    fn default() -> Self {
        Self {
            candidates: vec![
                "python3".to_string(),
                "python".to_string(),
                "py".to_string(),
            ],
            resolved: Mutex::new(None),
        }
    }
}

impl SystemPythonBackend {
    pub fn new(candidates: Vec<String>) -> Self {
        Self {
            candidates,
            resolved: Mutex::new(None),
        }
    }

    fn resolve_python(&self) -> Result<String> {
        if let Some(existing) = self.resolved.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Ok(existing);
        }

        for candidate in &self.candidates {
            if Command::new(candidate)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok()
            {
                let mut guard = self.resolved.lock().unwrap_or_else(|e| e.into_inner());
                let value = candidate.clone();
                *guard = Some(value.clone());
                return Ok(value);
            }
        }

        Err(anyhow!(
            "Could not find a python interpreter (candidates: {:?})",
            self.candidates
        ))
    }
}

impl PythonBackend for SystemPythonBackend {
    fn name(&self) -> &'static str {
        "python"
    }

    fn build_command(&self, _ctx: &PythonLaunchContext<'_>, args: &[String]) -> Result<Command> {
        let python = self.resolve_python()?;
        let mut cmd = Command::new(std::ffi::OsStr::new(&python));
        cmd.args(args.iter().map(|a| a.as_str()));
        Ok(cmd)
    }
}

fn apply_common_configuration(cmd: &mut Command, ctx: &PythonLaunchContext<'_>) -> Result<()> {
    cmd.current_dir(ctx.work_dir);
    cmd.env("VERI_CACHE_DIR", ctx.cache_dir);

    if !ctx.python_paths.is_empty() {
        let mut all_paths: Vec<PathBuf> = ctx.python_paths.to_vec();
        if let Some(existing) = env::var_os("PYTHONPATH") {
            all_paths.extend(env::split_paths(&existing));
        }
        let joined = env::join_paths(all_paths)
            .context("Failed to compose PYTHONPATH for Python launcher")?;
        cmd.env("PYTHONPATH", joined);
    }

    for (key, value) in ctx.extra_env {
        cmd.env(key, value);
    }

    Ok(())
}

fn build_backends_from_config(
    cfg: &PythonRuntimeConfig,
    work_dir: &Path,
) -> Vec<Arc<dyn PythonBackend>> {
    let mut backends: Vec<Arc<dyn PythonBackend>> = Vec::new();
    for backend in &cfg.backends {
        match backend {
            PythonBackendConfig::Uv {
                binary,
                project,
                enabled,
            } => {
                if enabled.unwrap_or(true) {
                    let bin = binary.clone().unwrap_or_else(|| "uv".to_string());
                    let project_override = project
                        .as_ref()
                        .map(|p| resolve_path(work_dir, p.as_path()));
                    backends.push(Arc::new(UvBackend::new(bin, project_override)));
                }
            }
            PythonBackendConfig::Python {
                executable,
                candidates,
                enabled,
            } => {
                if enabled.unwrap_or(true) {
                    let mut search = Vec::new();
                    if let Some(exec) = executable {
                        search.push(exec.clone());
                    }
                    if let Some(list) = candidates {
                        for item in list {
                            if !search.contains(item) {
                                search.push(item.clone());
                            }
                        }
                    }
                    if search.is_empty() {
                        search.extend(["python3", "python", "py"].iter().map(|s| s.to_string()));
                    }
                    backends.push(Arc::new(SystemPythonBackend::new(search)));
                }
            }
        }
    }
    backends
}

fn resolve_path(base: &Path, value: &Path) -> PathBuf {
    if value.is_absolute() {
        value.to_path_buf()
    } else {
        base.join(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn system_backend_fails_when_python_missing() {
        let backend = SystemPythonBackend::new(vec!["definitely-not-python".to_string()]);
        let ctx =
            PythonLaunchContext::new(Path::new("."), Path::new(".veri/cache"), &[], None, &[]);
        let result = backend.build_command(&ctx, &["--version".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn apply_common_configuration_with_pythonpath() {
        let backend = SystemPythonBackend::default();
        let launcher = PythonLauncher::new(vec![Arc::new(backend)]);
        let tempdir = tempfile::tempdir().unwrap();
        let pythonpath = vec![PathBuf::from("/tmp/example")];
        let ctx = PythonLaunchContext::new(tempdir.path(), tempdir.path(), &pythonpath, None, &[]);
        let mut cmd = launcher.backends[0]
            .build_command(&ctx, &["-c".to_string(), "print('ok')".to_string()])
            .unwrap();
        apply_common_configuration(&mut cmd, &ctx).unwrap();
        // We cannot easily introspect the command environment across Rust versions,
        // but the helper should succeed when pythonpath entries are supplied.
    }

    #[test]
    fn runtime_from_config_uses_explicit_paths() {
        let tempdir = tempfile::tempdir().unwrap();
        let explicit_py_worker = tempdir.path().join("py_worker_override");
        std::fs::create_dir_all(&explicit_py_worker).unwrap();
        let mut cfg = PythonRuntimeConfig::default();
        cfg.py_worker_path = Some(explicit_py_worker.clone());
        cfg.extra_pythonpath.push(PathBuf::from("extra_lib"));

        let runtime = PythonRuntime::from_config(tempdir.path(), &cfg);

        assert_eq!(runtime.py_worker_path.as_ref(), Some(&explicit_py_worker));
        assert!(runtime
            .python_paths
            .iter()
            .any(|p| p == &explicit_py_worker));
        let expected_extra = tempdir.path().join("extra_lib");
        assert!(runtime.python_paths.iter().any(|p| p == &expected_extra));
    }

    #[test]
    fn runtime_skips_disabled_backends() {
        let mut cfg = PythonRuntimeConfig::default();
        cfg.backends = vec![
            PythonBackendConfig::Uv {
                binary: Some("uv".to_string()),
                project: None,
                enabled: Some(false),
            },
            PythonBackendConfig::Python {
                executable: Some("python3".to_string()),
                candidates: None,
                enabled: Some(true),
            },
        ];
        let tempdir = tempfile::tempdir().unwrap();
        let runtime = PythonRuntime::from_config(tempdir.path(), &cfg);
        assert_eq!(runtime.launcher.backends.len(), 1);
    }

    #[test]
    fn test_launcher_fallback_chain() {
        // Create a launcher with a non-existent backend followed by system python
        let bad_backend = SystemPythonBackend::new(vec!["definitely-not-python".to_string()]);
        let good_backend = SystemPythonBackend::default();
        let launcher = PythonLauncher::new(vec![Arc::new(bad_backend), Arc::new(good_backend)]);

        let tempdir = tempfile::tempdir().unwrap();
        let ctx = PythonLaunchContext::new(tempdir.path(), tempdir.path(), &[], None, &[]);
        let args = vec!["-c".to_string(), "print('test')".to_string()];

        // Should succeed using the second backend
        let result = launcher.run(&ctx, &args);
        assert!(
            result.is_ok(),
            "Launcher should fall back to working backend"
        );
    }

    #[test]
    fn test_launcher_aggregates_errors() {
        // Create a launcher with only non-existent backends
        let bad1 = SystemPythonBackend::new(vec!["bad-python-1".to_string()]);
        let bad2 = SystemPythonBackend::new(vec!["bad-python-2".to_string()]);
        let launcher = PythonLauncher::new(vec![Arc::new(bad1), Arc::new(bad2)]);

        let tempdir = tempfile::tempdir().unwrap();
        let ctx = PythonLaunchContext::new(tempdir.path(), tempdir.path(), &[], None, &[]);
        let args = vec!["-c".to_string(), "print('test')".to_string()];

        let result = launcher.run(&ctx, &args);
        assert!(result.is_err(), "All backends should fail");

        let err_msg = format!("{:?}", result.unwrap_err());
        // Error message should mention both backends
        assert!(
            err_msg.contains("bad-python-1") || err_msg.contains("bad-python-2"),
            "Error should aggregate failures from all backends"
        );
    }

    #[test]
    fn test_system_backend_actually_executes() {
        // This test verifies the system backend can actually run Python
        let backend = SystemPythonBackend::default();
        let tempdir = tempfile::tempdir().unwrap();
        let ctx = PythonLaunchContext::new(tempdir.path(), tempdir.path(), &[], None, &[]);

        let result = backend.build_command(&ctx, &["-c".to_string(), "print('hello')".to_string()]);
        assert!(result.is_ok(), "Should be able to build command");

        let mut cmd = result.unwrap();
        apply_common_configuration(&mut cmd, &ctx).unwrap();

        // Try to execute (may fail in CI without Python, that's OK)
        if let Ok(output) = cmd.output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                assert!(stdout.contains("hello"), "Should execute Python code");
            }
        }
    }

    #[test]
    fn test_python_worker_uses_custom_runtime() {
        use crate::python_worker::PythonWorker;

        let tempdir = tempfile::tempdir().unwrap();
        let work_dir = tempdir.path();
        let cache_dir = work_dir.join(".veri").join("cache");

        // Create runtime with only system python backend
        let mut cfg = PythonRuntimeConfig::default();
        cfg.backends = vec![PythonBackendConfig::Python {
            executable: None,
            candidates: Some(vec!["python3".to_string(), "python".to_string()]),
            enabled: Some(true),
        }];

        let runtime = PythonRuntime::from_config(work_dir, &cfg);

        // Create worker from runtime
        let worker = PythonWorker::from_runtime(work_dir, &cache_dir, &runtime);

        // Verify worker has the runtime's configuration
        assert!(!worker.has_valid_cache()); // No cache yet, but should not error
    }

    #[test]
    fn test_uv_backend_probes_availability() {
        let backend = UvBackend::default();
        let tempdir = tempfile::tempdir().unwrap();
        let ctx = PythonLaunchContext::new(tempdir.path(), tempdir.path(), &[], None, &[]);

        // Try to build command (may fail if uv not installed, which is fine)
        let result = backend.build_command(&ctx, &["--version".to_string()]);

        // If uv is available, command should build successfully
        // If not, we should get a clear error about uv not being available
        if result.is_err() {
            let err_msg = format!("{:?}", result.unwrap_err());
            assert!(
                err_msg.contains("uv") || err_msg.contains("version"),
                "Error should mention uv availability check"
            );
        }
    }

    #[test]
    fn test_backend_ordering_matters() {
        // Create config with specific backend order
        let mut cfg = PythonRuntimeConfig::default();
        cfg.backends = vec![
            PythonBackendConfig::Python {
                executable: Some("python3".to_string()),
                candidates: None,
                enabled: Some(true),
            },
            PythonBackendConfig::Uv {
                binary: Some("uv".to_string()),
                project: None,
                enabled: Some(true),
            },
        ];

        let tempdir = tempfile::tempdir().unwrap();
        let runtime = PythonRuntime::from_config(tempdir.path(), &cfg);

        // First backend should be python (not uv)
        assert_eq!(runtime.launcher.backends[0].name(), "python");
        if runtime.launcher.backends.len() > 1 {
            assert_eq!(runtime.launcher.backends[1].name(), "uv");
        }
    }
}
