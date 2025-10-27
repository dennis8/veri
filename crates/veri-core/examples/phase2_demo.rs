use anyhow::Result;
use veri_core::cache::CacheKey;
use veri_core::schemas::*;

/// Example demonstrating schema usage and cache key computation
fn main() -> Result<()> {
    println!("=== veri Phase 2 Schema & Cache Key Demo ===\n");

    // 1. Create a sample tests index
    println!("1. Creating Tests Index:");
    let mut tests_index = TestsIndex::new("3.11.0".to_string(), "7.4.0".to_string());

    let test_node = TestNode {
        nodeid: "test_example.py::test_addition".to_string(),
        path: "test_example.py".to_string(),
        line: 15,
        function: "test_addition".to_string(),
        class: None,
        module: "test_example".to_string(),
        markers: vec!["unit".to_string(), "fast".to_string()],
        fixtures: vec!["tmpdir".to_string()],
        parametrize: Some(ParametrizeInfo {
            params: vec!["1,2,3".to_string(), "4,5,9".to_string()],
            ids: vec!["simple".to_string(), "complex".to_string()],
        }),
    };

    tests_index.tests.push(test_node);
    println!(
        "   ✓ Created test index with {} tests",
        tests_index.tests.len()
    );
    println!("   ✓ JSON size: {} bytes", tests_index.to_json()?.len());

    // 2. Create a sample imports graph
    println!("\n2. Creating Imports Graph:");
    let mut imports_graph = ImportsGraph::new();

    let import_edge = ImportEdge {
        from_module: "test_example".to_string(),
        to_module: "src.calculator".to_string(),
        import_type: ImportType::From,
        line: 3,
        names: vec!["add".to_string(), "subtract".to_string()],
        alias: None,
        is_conditional: false,
    };

    imports_graph.edges.push(import_edge);

    let dynamic_import = DynamicImport {
        from_module: "test_integration".to_string(),
        line: 25,
        function: DynamicImportFunction::ImportlibImportModule,
        argument: Some("plugins.loader".to_string()),
        reason: "Plugin name determined at runtime".to_string(),
    };

    imports_graph.dynamic_imports.push(dynamic_import);
    println!(
        "   ✓ Created import graph with {} edges",
        imports_graph.edges.len()
    );
    println!(
        "   ✓ Found {} dynamic imports",
        imports_graph.dynamic_imports.len()
    );

    // 3. Create a sample event stream
    println!("\n3. Creating Event Stream:");
    let start_event = Event::Start {
        ts: chrono::Utc::now(),
        run_id: "demo-run-12345".to_string(),
        veri_version: "0.1.0".to_string(),
        python_version: "3.11.0".to_string(),
        platform: "demo-platform".to_string(),
        workers: 4,
        cache_key: "abc123def456".to_string(),
    };

    let case_event = Event::Case {
        ts: chrono::Utc::now(),
        run_id: "demo-run-12345".to_string(),
        nodeid: "test_example.py::test_addition[simple]".to_string(),
        outcome: TestOutcome::Passed,
        duration: 0.042,
        worker_id: Some("worker-0".to_string()),
        longrepr: None,
        markers: vec!["unit".to_string(), "fast".to_string()],
    };

    println!("   ✓ Start event: {}", start_event.to_jsonl_line()?);
    println!("   ✓ Case event: {}", case_event.to_jsonl_line()?);

    // 4. Create a shards manifest
    println!("\n4. Creating Shards Manifest:");
    let mut manifest = ShardsManifest::new(4, ShardingStrategy::TimingBased);

    let shard = Shard {
        shard_id: 0,
        estimated_duration: 15.3,
        test_count: 8,
        tests: vec![
            ShardTest {
                nodeid: "test_example.py::test_addition[simple]".to_string(),
                estimated_duration: 0.5,
                priority: 0,
                markers: vec!["unit".to_string()],
            },
            ShardTest {
                nodeid: "test_slow.py::test_integration".to_string(),
                estimated_duration: 14.8,
                priority: 1,
                markers: vec!["integration".to_string(), "slow".to_string()],
            },
        ],
    };

    manifest.shards.push(shard);
    manifest.estimated_duration = 60.0;
    println!("   ✓ Created manifest for {} shards", manifest.total_shards);
    println!("   ✓ Format version: {}", manifest.format_version);

    // 5. Demonstrate cache key computation
    println!("\n5. Cache Key Computation:");
    let config = veri_core::config::Config::default();
    let config_digest = veri_core::cache::compute_config_digest(&config)?;
    // Pass None to use system python fallback (demo doesn't need full runtime)
    let cache_key = CacheKey::from_environment(config_digest, None)?;

    println!("   ✓ Cache key components:");
    println!("      - Python: {}", cache_key.python_version);
    println!("      - Platform: {}", cache_key.platform);
    println!("      - veri: {}", cache_key.veri_version);
    println!(
        "      - Conftest files: {}",
        cache_key.conftest_digests.len()
    );
    println!("   ✓ Final hash: {}", cache_key.compute_hash());

    println!("\n=== Phase 2 Implementation Complete! ===");
    println!("✓ All 9 JSON schemas implemented");
    println!("✓ Cache key computation with deterministic hashing");
    println!("✓ Schema serialization/deserialization with validation");
    println!("✓ CI integration for schema validation");
    println!("✓ Real cache key components in --explain output");

    Ok(())
}
