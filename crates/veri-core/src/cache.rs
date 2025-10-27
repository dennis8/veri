use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;

use crate::python_launcher::PythonLauncher;

/// Cache key components for deterministic caching
#[derive(Debug, Clone)]
pub struct CacheKey {
    pub python_version: String,
    pub platform: String,
    pub veri_version: String,
    pub uv_lock_digest: Option<String>,
    pub site_packages_digest: Option<String>,
    pub pytest_version: String,
    pub plugins: Vec<String>,
    pub conftest_digests: HashMap<String, String>,
    pub veri_config_digest: String,
}

#[derive(Debug, Clone, Deserialize)]
struct PythonEnvironmentSnapshot {
    python_version: String,
    #[serde(default)]
    pytest_version: Option<String>,
    #[serde(default)]
    pytest_plugins: Vec<String>,
    #[serde(default)]
    site_packages_digest: Option<String>,
}

static PYTHON_ENV_SNAPSHOT: OnceLock<Mutex<Option<PythonEnvironmentSnapshot>>> = OnceLock::new();

impl CacheKey {
    /// Create a cache key from the current environment
    ///
    /// If `launcher` is provided, uses it to probe the Python environment.
    /// Otherwise falls back to system python discovery.
    pub fn from_environment(
        config_digest: String,
        launcher: Option<&PythonLauncher>,
    ) -> Result<Self> {
        Ok(CacheKey {
            python_version: Self::get_python_version(launcher)?,
            platform: Self::get_platform(),
            veri_version: env!("CARGO_PKG_VERSION").to_string(),
            uv_lock_digest: Self::get_uv_lock_digest()?,
            site_packages_digest: Self::get_site_packages_digest(launcher)?,
            pytest_version: Self::get_pytest_version(launcher)?,
            plugins: Self::get_pytest_plugins(launcher)?,
            conftest_digests: Self::get_conftest_digests()?,
            veri_config_digest: config_digest,
        })
    }

    /// Compute the cache key hash
    pub fn compute_hash(&self) -> String {
        let mut hasher = Sha256::new();

        // Hash each component in deterministic order
        hasher.update(b"python_version:");
        hasher.update(self.python_version.as_bytes());
        hasher.update(b"|");

        hasher.update(b"platform:");
        hasher.update(self.platform.as_bytes());
        hasher.update(b"|");

        hasher.update(b"veri_version:");
        hasher.update(self.veri_version.as_bytes());
        hasher.update(b"|");

        if let Some(ref digest) = self.uv_lock_digest {
            hasher.update(b"uv_lock_digest:");
            hasher.update(digest.as_bytes());
        }
        hasher.update(b"|");

        if let Some(ref digest) = self.site_packages_digest {
            hasher.update(b"site_packages_digest:");
            hasher.update(digest.as_bytes());
        }
        hasher.update(b"|");

        hasher.update(b"pytest_version:");
        hasher.update(self.pytest_version.as_bytes());
        hasher.update(b"|");

        hasher.update(b"plugins:");
        for plugin in &self.plugins {
            hasher.update(plugin.as_bytes());
            hasher.update(b",");
        }
        hasher.update(b"|");

        hasher.update(b"conftest_digests:");
        let mut conftest_pairs: Vec<_> = self.conftest_digests.iter().collect();
        conftest_pairs.sort_by_key(|(path, _)| path.as_str());
        for (path, digest) in conftest_pairs {
            hasher.update(path.as_bytes());
            hasher.update(b":");
            hasher.update(digest.as_bytes());
            hasher.update(b",");
        }
        hasher.update(b"|");

        hasher.update(b"veri_config_digest:");
        hasher.update(self.veri_config_digest.as_bytes());

        format!("{:x}", hasher.finalize())
    }

    /// Get Python version from current interpreter
    fn get_python_version(launcher: Option<&PythonLauncher>) -> Result<String> {
        let python_env = Self::python_env_snapshot(launcher)?;
        Ok(python_env.python_version)
    }

    /// Get platform identifier
    fn get_platform() -> String {
        format!("{}-{}", env::consts::OS, env::consts::ARCH)
    }

    /// Get uv.lock digest if present
    fn get_uv_lock_digest() -> Result<Option<String>> {
        let lock_path = Path::new("uv.lock");
        if lock_path.exists() {
            let content = fs::read(lock_path).context("Failed to read uv.lock")?;
            let digest = format!("{:x}", Sha256::digest(&content));
            Ok(Some(digest))
        } else {
            Ok(None)
        }
    }

    /// Get site-packages digest
    fn get_site_packages_digest(launcher: Option<&PythonLauncher>) -> Result<Option<String>> {
        let python_env = Self::python_env_snapshot(launcher)?;
        Ok(python_env.site_packages_digest)
    }

    /// Get pytest version from current environment
    fn get_pytest_version(launcher: Option<&PythonLauncher>) -> Result<String> {
        let python_env = Self::python_env_snapshot(launcher)?;
        Ok(python_env
            .pytest_version
            .unwrap_or_else(|| "pytest-not-found".to_string()))
    }

    /// Get list of installed pytest plugins
    fn get_pytest_plugins(launcher: Option<&PythonLauncher>) -> Result<Vec<String>> {
        let python_env = Self::python_env_snapshot(launcher)?;
        Ok(python_env.pytest_plugins)
    }

    /// Get digests of all conftest.py files
    fn get_conftest_digests() -> Result<HashMap<String, String>> {
        let mut digests = HashMap::new();

        // Find all conftest.py files in current directory and subdirectories
        if let Ok(entries) = Self::find_conftest_files(Path::new(".")) {
            for path in entries {
                if let Ok(content) = fs::read(&path) {
                    let digest = format!("{:x}", Sha256::digest(&content));
                    let path_str = path.to_string_lossy().to_string();
                    digests.insert(path_str, digest);
                }
            }
        }

        Ok(digests)
    }

    fn python_env_snapshot(launcher: Option<&PythonLauncher>) -> Result<PythonEnvironmentSnapshot> {
        let storage = PYTHON_ENV_SNAPSHOT.get_or_init(|| Mutex::new(None));

        {
            let guard = storage
                .lock()
                .expect("python environment snapshot mutex poisoned");
            if let Some(snapshot) = &*guard {
                return Ok(snapshot.clone());
            }
        }

        let snapshot = Self::collect_python_environment_snapshot(launcher)?;

        let mut guard = storage
            .lock()
            .expect("python environment snapshot mutex poisoned");
        *guard = Some(snapshot.clone());

        Ok(snapshot)
    }

    fn collect_python_environment_snapshot(
        launcher: Option<&PythonLauncher>,
    ) -> Result<PythonEnvironmentSnapshot> {
        let script = r#"
import json
import hashlib
import platform

result = {
    "python_version": platform.python_version(),
    "pytest_version": None,
    "pytest_plugins": [],
    "site_packages_digest": None,
}

# Determine pytest version
try:
    import pytest
    result["pytest_version"] = getattr(pytest, "__version__", None)
except ModuleNotFoundError:
    result["pytest_version"] = None
except Exception:
    result["pytest_version"] = None

plugins = set()
package_records = []

def add_package(name, version, location):
    package_records.append({
        "name": name or "",
        "version": version or "",
        "location": location or "",
    })

def add_plugin(name, version):
    if not name:
        return
    display = f"{name}=={version}" if version else name
    plugins.add(display)

try:
    import pkg_resources  # type: ignore
except Exception:
    pkg_resources = None

if pkg_resources is not None:
    try:
        for dist in pkg_resources.working_set:
            name = dist.project_name
            version = dist.version
            location = dist.location

            add_package(name, version, location)

        # Detect pytest plugins only via pytest11 entry points
        try:
            for entry_point in pkg_resources.iter_entry_points("pytest11"):
                dist = getattr(entry_point, "dist", None)
                if dist is not None:
                    add_plugin(dist.project_name, dist.version)
                else:
                    add_plugin(entry_point.module_name, None)
        except Exception:
            pass
    except Exception:
        pass

if not package_records or not plugins:
    try:
        try:
            from importlib import metadata
        except ImportError:
            import importlib_metadata as metadata  # type: ignore

        for dist in metadata.distributions():
            name = getattr(dist, "metadata", {}).get("Name") if hasattr(dist, "metadata") else None
            if not name and hasattr(dist, "name"):
                name = getattr(dist, "name")
            version = getattr(dist, "version", None)

            add_package(name, version, "")

        # Detect pytest plugins only via pytest11 entry points
        try:
            entry_points = metadata.entry_points()
            if hasattr(entry_points, "select"):
                selected = entry_points.select(group="pytest11")
            else:
                selected = entry_points.get("pytest11", [])
            for entry in selected:
                dist = getattr(entry, "dist", None)
                if dist is not None:
                    add_plugin(getattr(dist, "name", None), getattr(dist, "version", None))
                else:
                    add_plugin(getattr(entry, "name", None), None)
        except Exception:
            pass
    except Exception:
        pass

seen_records = set()
deduped_records = []
for record in package_records:
    key = (record["name"].lower(), record["version"], record["location"])
    if key in seen_records:
        continue
    seen_records.add(key)
    deduped_records.append(record)

deduped_records.sort(key=lambda item: (item["name"].lower(), item["version"], item["location"]))
plugin_list = sorted(list(plugins), key=lambda item: item.encode('ascii', 'ignore').decode().lower())

if deduped_records:
    digest_input = json.dumps(deduped_records, sort_keys=True, separators=(",", ":")).encode("utf-8")
    result["site_packages_digest"] = hashlib.sha256(digest_input).hexdigest()

result["pytest_plugins"] = plugin_list

print(json.dumps(result))
"#;

        let output = if let Some(launcher) = launcher {
            // Use the provided launcher (matches worker environment)
            use crate::python_launcher::PythonLaunchContext;
            let work_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let cache_dir = work_dir.join(".veri").join("cache");
            let ctx = PythonLaunchContext::new(&work_dir, &cache_dir, &[], None, &[]);
            let args = vec!["-c".to_string(), script.to_string()];
            launcher
                .run(&ctx, &args)
                .context("Failed to run Python environment probe via launcher")?
        } else {
            // Fallback to system python discovery
            use std::process::Command;
            let python = Self::find_python_executable()?;
            Command::new(&python)
                .args(["-c", script])
                .output()
                .with_context(|| format!("Failed to run Python interpreter '{}'", python))?
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "Python environment probe failed (status {}): {}",
                output.status,
                stderr.trim()
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut snapshot: PythonEnvironmentSnapshot = serde_json::from_str(&stdout)
            .context("Failed to parse Python environment snapshot JSON")?;

        // Ensure deterministic ordering even if the Python helper returned duplicates
        // Use ASCII lowercasing to match Python's behavior
        snapshot
            .pytest_plugins
            .sort_by(|a, b| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()));
        snapshot.pytest_plugins.dedup();

        // Deduplicate plugins that appear both with and without versions
        // Keep only the versioned form when both exist (e.g., "foo==1.0" over "foo")
        let mut seen_names = std::collections::HashSet::new();
        let mut deduplicated = Vec::new();

        // First pass: collect all versioned plugins
        for plugin in &snapshot.pytest_plugins {
            if plugin.contains("==") {
                let name = plugin.split("==").next().unwrap();
                seen_names.insert(name.to_string());
                deduplicated.push(plugin.clone());
            }
        }

        // Second pass: add unversioned plugins only if no versioned form exists
        for plugin in &snapshot.pytest_plugins {
            if !plugin.contains("==") && !seen_names.contains(plugin) {
                deduplicated.push(plugin.clone());
            }
        }

        snapshot.pytest_plugins = deduplicated;

        Ok(snapshot)
    }

    fn find_python_executable() -> Result<String> {
        use std::process::Command;
        let candidates = ["python3", "python", "py"];
        for candidate in candidates {
            let result = Command::new(candidate).arg("--version").output();
            if let Ok(output) = result {
                if output.status.success() {
                    return Ok(candidate.to_string());
                }
            }
        }

        Err(anyhow!(
            "Could not find a Python interpreter (checked python3, python, py)"
        ))
    }

    /// Recursively find all conftest.py files
    fn find_conftest_files(dir: &Path) -> Result<Vec<PathBuf>> {
        let mut visited = std::collections::HashSet::new();
        Self::find_conftest_files_impl(dir, &mut visited)
    }

    fn find_conftest_files_impl(
        dir: &Path,
        visited: &mut std::collections::HashSet<PathBuf>,
    ) -> Result<Vec<PathBuf>> {
        let mut conftest_files = Vec::new();

        if !dir.is_dir() {
            return Ok(conftest_files);
        }

        // Protect against symlink loops by tracking canonical paths
        let canonical = match dir.canonicalize() {
            Ok(p) => p,
            Err(_) => return Ok(conftest_files), // Skip if canonicalization fails
        };

        if !visited.insert(canonical) {
            return Ok(conftest_files); // Already visited, avoid loop
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path.file_name() == Some(std::ffi::OsStr::new("conftest.py")) {
                conftest_files.push(path);
            } else if path.is_dir() {
                // Skip hidden directories and common non-source directories
                if let Some(dir_name) = path.file_name() {
                    let dir_name = dir_name.to_string_lossy();
                    if !dir_name.starts_with('.')
                        && dir_name != "node_modules"
                        && dir_name != "__pycache__"
                        && dir_name != "target"
                        && dir_name != "venv"
                        && dir_name != ".venv"
                        && dir_name != "dist"
                        && dir_name != "build"
                        && dir_name != ".git"
                    {
                        conftest_files.extend(Self::find_conftest_files_impl(&path, visited)?);
                    }
                }
            }
        }

        Ok(conftest_files)
    }

    /// Print cache key components for --explain
    pub fn print_explanation(&self) {
        println!("Cache key components:");
        println!("  python_version: {}", self.python_version);
        println!("  platform: {}", self.platform);
        println!("  veri_version: {}", self.veri_version);

        if let Some(ref digest) = self.uv_lock_digest {
            println!("  uv_lock_digest: {}", digest);
        } else {
            println!("  uv_lock_digest: (not found)");
        }

        if let Some(ref digest) = self.site_packages_digest {
            println!("  site_packages_digest: {}", digest);
        } else {
            println!("  site_packages_digest: (not computed)");
        }

        println!("  pytest_version: {}", self.pytest_version);

        if self.plugins.is_empty() {
            println!("  plugins: []");
        } else {
            println!("  plugins: [{}]", self.plugins.join(", "));
        }

        if self.conftest_digests.is_empty() {
            println!("  conftest_digests: {{}}");
        } else {
            println!("  conftest_digests: {{");
            let mut sorted_conftest: Vec<_> = self.conftest_digests.iter().collect();
            sorted_conftest.sort_by_key(|(path, _)| path.as_str());
            for (path, digest) in sorted_conftest {
                println!("    \"{}\": \"{}\"", path, digest);
            }
            println!("  }}");
        }

        println!("  veri_config_digest: {}", self.veri_config_digest);

        let cache_hash = self.compute_hash();
        println!("  computed_hash: {}", cache_hash);
    }
}

/// Compute digest of configuration for cache key
pub fn compute_config_digest(config: &crate::config::Config) -> Result<String> {
    // Serialize config to JSON and hash it
    let config_json = serde_json::to_string(config).context("Failed to serialize config")?;

    let digest = format!("{:x}", Sha256::digest(config_json.as_bytes()));
    Ok(digest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_cache_key_hash_deterministic() {
        let cache_key = CacheKey {
            python_version: "3.11.0".to_string(),
            platform: "linux-x86_64".to_string(),
            veri_version: "0.1.0".to_string(),
            uv_lock_digest: Some("abc123".to_string()),
            site_packages_digest: Some("def456".to_string()),
            pytest_version: "7.4.0".to_string(),
            plugins: vec!["pytest-cov".to_string()],
            conftest_digests: {
                let mut map = HashMap::new();
                map.insert("conftest.py".to_string(), "ghi789".to_string());
                map
            },
            veri_config_digest: "config123".to_string(),
        };

        let hash1 = cache_key.compute_hash();
        let hash2 = cache_key.compute_hash();
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64); // SHA-256 hex string
    }

    #[test]
    fn test_cache_key_different_for_different_configs() {
        let cache_key1 = CacheKey {
            python_version: "3.11.0".to_string(),
            platform: "linux-x86_64".to_string(),
            veri_version: "0.1.0".to_string(),
            uv_lock_digest: None,
            site_packages_digest: None,
            pytest_version: "7.4.0".to_string(),
            plugins: Vec::new(),
            conftest_digests: HashMap::new(),
            veri_config_digest: "config1".to_string(),
        };

        let cache_key2 = CacheKey {
            python_version: "3.11.0".to_string(),
            platform: "linux-x86_64".to_string(),
            veri_version: "0.1.0".to_string(),
            uv_lock_digest: None,
            site_packages_digest: None,
            pytest_version: "7.4.0".to_string(),
            plugins: Vec::new(),
            conftest_digests: HashMap::new(),
            veri_config_digest: "config2".to_string(),
        };

        assert_ne!(cache_key1.compute_hash(), cache_key2.compute_hash());
    }

    #[test]
    fn test_find_conftest_files() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // Create some conftest.py files
        fs::write(root.join("conftest.py"), "# root conftest").unwrap();
        fs::create_dir(root.join("tests")).unwrap();
        fs::write(root.join("tests").join("conftest.py"), "# tests conftest").unwrap();
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src").join("conftest.py"), "# src conftest").unwrap();

        let conftest_files = CacheKey::find_conftest_files(root).unwrap();
        assert_eq!(conftest_files.len(), 3);

        // All should be named conftest.py
        for file in &conftest_files {
            assert_eq!(file.file_name().unwrap(), "conftest.py");
        }
    }
}
