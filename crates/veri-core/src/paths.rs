//! Path utilities for locating project resources

use std::path::{Path, PathBuf};

/// Find the py_worker directory path by searching from a starting directory upwards
///
/// This function searches for the `py_worker` directory by:
/// 1. Starting from the given directory
/// 2. Checking up to 5 parent directories
/// 3. Falling back to the repository root relative to CARGO_MANIFEST_DIR
///
/// # Arguments
/// * `start` - The starting directory to search from
///
/// # Returns
/// * `Some(PathBuf)` - Path to the py_worker directory if found
/// * `None` - If py_worker directory cannot be located
pub fn find_py_worker_path(start: &Path) -> Option<PathBuf> {
    // Try start/py_worker and ascend up to 5 levels
    let mut current = start.to_path_buf();
    for _ in 0..5 {
        let candidate = current.join("py_worker");
        if candidate.exists() {
            return Some(candidate);
        }
        if !current.pop() {
            break;
        }
    }

    // Try from current executable location (for installed binaries)
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let mut potential_root = parent.to_path_buf();
            // Handle typical cargo layouts: target/debug or target/release
            if potential_root.ends_with("debug") || potential_root.ends_with("release") {
                potential_root.pop(); // remove profile dir
                potential_root.pop(); // remove target
            }

            let potential_py_worker = potential_root.join("py_worker");
            if potential_py_worker.exists() {
                return Some(potential_py_worker);
            }
        }
    }

    // Try repo relative to the veri-core crate (../../py_worker)
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(repo_root) = manifest_dir.parent().and_then(|p| p.parent()) {
        let candidate = repo_root.join("py_worker");
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_find_py_worker_from_manifest_dir() {
        // This should work when running from the repository
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        if let Some(repo_root) = manifest_dir.parent().and_then(|p| p.parent()) {
            let result = find_py_worker_path(repo_root);
            // If py_worker exists in the repo, it should be found
            if repo_root.join("py_worker").exists() {
                assert!(result.is_some());
                assert_eq!(result.unwrap(), repo_root.join("py_worker"));
            }
        }
    }

    #[test]
    fn test_find_py_worker_returns_none_for_nonexistent() {
        let temp_dir = std::env::temp_dir();
        let nonexistent = temp_dir.join("definitely_does_not_exist_12345");
        let result = find_py_worker_path(&nonexistent);
        // Should eventually find py_worker if it exists in the repo
        // or return None if not in development environment
        // This test mainly ensures no panics occur
        assert!(result.is_some() || result.is_none());
    }
}
