//! Integration test for Phase 3 Python worker functionality

use std::path::PathBuf;
use tempfile::TempDir;
use crate::python_worker::PythonWorker;

#[test]
fn test_python_worker_creation() {
    let temp_dir = TempDir::new().unwrap();
    let work_dir = temp_dir.path().to_path_buf();
    let cache_dir = work_dir.join(".veri").join("cache");
    
    let worker = PythonWorker::new(&work_dir, &cache_dir);
    
    // Check paths are set correctly
    assert_eq!(worker.tests_index_path(), cache_dir.join("tests.index.json"));
    assert_eq!(worker.markers_index_path(), cache_dir.join("markers.index.json"));
    assert!(!worker.has_valid_cache()); // No cache files exist yet
}