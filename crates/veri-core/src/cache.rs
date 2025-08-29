use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

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

impl CacheKey {
    /// Create a cache key from the current environment
    pub fn from_environment(config_digest: String) -> Result<Self> {
        Ok(CacheKey {
            python_version: Self::get_python_version()?,
            platform: Self::get_platform(),
            veri_version: env!("CARGO_PKG_VERSION").to_string(),
            uv_lock_digest: Self::get_uv_lock_digest()?,
            site_packages_digest: Self::get_site_packages_digest()?,
            pytest_version: Self::get_pytest_version()?,
            plugins: Self::get_pytest_plugins()?,
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
    fn get_python_version() -> Result<String> {
        // For now, return a placeholder - in Phase 3 we'll get this from the Python worker
        Ok("3.11.0".to_string())
    }

    /// Get platform identifier
    fn get_platform() -> String {
        format!("{}-{}", env::consts::OS, env::consts::ARCH)
    }

    /// Get uv.lock digest if present
    fn get_uv_lock_digest() -> Result<Option<String>> {
        let lock_path = Path::new("uv.lock");
        if lock_path.exists() {
            let content = fs::read(lock_path)
                .context("Failed to read uv.lock")?;
            let digest = format!("{:x}", Sha256::digest(&content));
            Ok(Some(digest))
        } else {
            Ok(None)
        }
    }

    /// Get site-packages digest
    fn get_site_packages_digest() -> Result<Option<String>> {
        // For now, return None - in Phase 3 we'll implement proper site-packages scanning
        Ok(None)
    }

    /// Get pytest version from current environment
    fn get_pytest_version() -> Result<String> {
        // For now, return a placeholder - in Phase 3 we'll get this from the Python worker
        Ok("7.4.0".to_string())
    }

    /// Get list of installed pytest plugins
    fn get_pytest_plugins() -> Result<Vec<String>> {
        // For now, return empty list - in Phase 3 we'll scan for actual plugins
        Ok(Vec::new())
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

    /// Recursively find all conftest.py files
    fn find_conftest_files(dir: &Path) -> Result<Vec<PathBuf>> {
        let mut conftest_files = Vec::new();
        
        if !dir.is_dir() {
            return Ok(conftest_files);
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
                    if !dir_name.starts_with('.') && 
                       dir_name != "node_modules" && 
                       dir_name != "__pycache__" &&
                       dir_name != "target" {
                        conftest_files.extend(Self::find_conftest_files(&path)?);
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
    let config_json = serde_json::to_string(config)
        .context("Failed to serialize config")?;
    
    let digest = format!("{:x}", Sha256::digest(config_json.as_bytes()));
    Ok(digest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

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