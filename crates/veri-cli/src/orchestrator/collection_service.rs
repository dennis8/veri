use anyhow::Result;
use veri_core::import_graph::{ImportsGraph, ReverseDepsGraph, ModuleMap};
use veri_core::python_launcher::PythonRuntime;
use veri_core::python_worker::{PythonWorker, TestsIndex};
use veri_core::diagnostics::DiagnosticReporter;
use std::path::Path;
use std::time::Instant;

/// Result of test collection operation
#[derive(Debug)]
pub struct CollectionOutcome {
    /// Collected test index
    pub tests_index: TestsIndex,
    /// Import graphs (optional, built only for impact analysis) - (ImportsGraph, ReverseDepsGraph, ModuleMap)
    pub graphs: Option<(ImportsGraph, ReverseDepsGraph, ModuleMap)>,
    /// Time spent collecting tests (milliseconds)
    pub collection_time_ms: u64,
}

/// Trait for orchestrating test collection
pub trait CollectionOrchestrationService: Send + Sync {
    fn collect_or_load(
        &self,
        cli_all: bool,
        collection_paths: &[String],
        ignore_patterns: &[String],
        diagnostics: &mut DiagnosticReporter,
    ) -> Result<CollectionOutcome>;
}

/// Default implementation for test collection
pub struct DefaultCollectionService {
    work_dir: std::path::PathBuf,
    cache_dir: std::path::PathBuf,
    python_runtime: PythonRuntime,
}

impl DefaultCollectionService {
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

impl CollectionOrchestrationService for DefaultCollectionService {
    fn collect_or_load(
        &self,
        cli_all: bool,
        collection_paths: &[String],
        ignore_patterns: &[String],
        diagnostics: &mut DiagnosticReporter,
    ) -> Result<CollectionOutcome> {
        // Create a worker to perform collection
        let worker = PythonWorker::from_runtime(&self.work_dir, &self.cache_dir, &self.python_runtime);

        let collection_start = Instant::now();

        // Check if we need to collect tests (first run or --all)
        let needs_collection = cli_all || !worker.has_valid_cache();

        let (tests_index, collection_time_ms, graphs) = if needs_collection {
            println!("📋 Collecting tests...");

            // Collect tests
            let tests_index = worker.collect_tests(collection_paths, ignore_patterns)?;

            // Check for collection errors
            worker.check_collection_errors(&tests_index, diagnostics);

            println!("✅ Collected {} tests", tests_index.tests.len());

            if !tests_index.collection_errors.is_empty() {
                println!(
                    "⚠️  {} collection errors encountered",
                    tests_index.collection_errors.len()
                );
                for error in &tests_index.collection_errors {
                    eprintln!("  {}: {}", error.path, error.message);
                }
            }

            // Build import graph only when impact analysis is needed (not --all)
            let graphs = if !cli_all {
                println!("🔍 Building import graph...");
                let mut graph_builder =
                    veri_core::import_graph::ImportGraphBuilder::with_runtime(
                        &self.work_dir,
                        &self.cache_dir,
                        &self.python_runtime,
                    );
                let graphs_tuple = graph_builder.build_graphs()?;

                println!(
                    "✅ Built import graph with {} edges",
                    graphs_tuple.0.edges.len()
                );
                if !graphs_tuple.0.dynamic_imports.is_empty() {
                    println!(
                        "⚠️  {} dynamic imports detected",
                        graphs_tuple.0.dynamic_imports.len()
                    );
                }
                Some(graphs_tuple)
            } else {
                None
            };

            let collection_time_ms = collection_start.elapsed().as_millis() as u64;
            (tests_index, collection_time_ms, graphs)
        } else {
            // Load cached tests (no collection time measured)
            let tests_index = worker.collect_tests(collection_paths, ignore_patterns)?;
            (tests_index, 0, None)
        };

        Ok(CollectionOutcome {
            tests_index,
            graphs,
            collection_time_ms,
        })
    }
}

#[cfg(test)]
mod testing {
    use super::*;
    use chrono::Utc;
    use veri_core::schemas::{TestsIndex, TestNode};

    /// Mock collection service for testing
    pub struct MockCollectionService {
        pub test_count: usize,
        pub collection_time_ms: u64,
    }

    impl MockCollectionService {
        pub fn new(test_count: usize) -> Self {
            Self {
                test_count,
                collection_time_ms: 100,
            }
        }

        pub fn with_tests(test_count: usize) -> Self {
            Self::new(test_count)
        }
    }

    impl CollectionOrchestrationService for MockCollectionService {
        fn collect_or_load(
            &self,
            _cli_all: bool,
            _collection_paths: &[String],
            _ignore_patterns: &[String],
            _diagnostics: &mut DiagnosticReporter,
        ) -> Result<CollectionOutcome> {
            let tests_index = TestsIndex {
                version: "1.0".to_string(),
                generated_at: Utc::now(),
                python_version: "3.11".to_string(),
                pytest_version: "7.0".to_string(),
                tests: (0..self.test_count)
                    .map(|i| TestNode {
                        nodeid: format!("test_{}", i),
                        path: format!("test_{}.py", i),
                        line: i as u32,
                        function: format!("test_{}", i),
                        class: None,
                        module: "tests".to_string(),
                        markers: vec![],
                        fixtures: vec![],
                        parametrize: None,
                    })
                    .collect(),
                collection_errors: vec![],
            };

            Ok(CollectionOutcome {
                tests_index,
                graphs: None,
                collection_time_ms: self.collection_time_ms,
            })
        }
    }
}
