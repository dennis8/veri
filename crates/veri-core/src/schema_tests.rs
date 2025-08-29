#[cfg(test)]
mod schema_tests {
    use crate::schemas::*;
    use chrono::Utc;

    #[test]
    fn test_tests_index_serialization() {
        let mut index = TestsIndex::new("3.11.0".to_string(), "7.4.0".to_string());
        
        let test_node = TestNode {
            nodeid: "test_example.py::test_function".to_string(),
            path: "test_example.py".to_string(),
            line: 10,
            function: "test_function".to_string(),
            class: None,
            module: "test_example".to_string(),
            markers: vec!["unit".to_string()],
            fixtures: vec!["tmpdir".to_string()],
            parametrize: None,
        };
        
        index.tests.push(test_node);
        
        let json = index.to_json().unwrap();
        let parsed = TestsIndex::from_json(&json).unwrap();
        
        assert_eq!(parsed.tests.len(), 1);
        assert_eq!(parsed.tests[0].nodeid, "test_example.py::test_function");
        assert_eq!(parsed.tests[0].markers, vec!["unit"]);
    }

    #[test]
    fn test_module_map_serialization() {
        let mut module_map = ModuleMap::new();
        
        let module_info = ModuleInfo {
            module_name: "example.module".to_string(),
            is_package: false,
            is_namespace: false,
            parent_package: Some("example".to_string()),
            relative_path: "example/module.py".to_string(),
            digest: "abc123def456".to_string(),
        };
        
        module_map.modules.insert("example/module.py".to_string(), module_info);
        
        let json = module_map.to_json().unwrap();
        let parsed = ModuleMap::from_json(&json).unwrap();
        
        assert_eq!(parsed.modules.len(), 1);
        let module = parsed.modules.get("example/module.py").unwrap();
        assert_eq!(module.module_name, "example.module");
        assert!(!module.is_package);
    }

    #[test]
    fn test_imports_graph_serialization() {
        let mut graph = ImportsGraph::new();
        
        let edge = ImportEdge {
            from_module: "test_module".to_string(),
            to_module: "target_module".to_string(),
            import_type: ImportType::From,
            line: 5,
            names: vec!["function".to_string()],
            alias: None,
            is_conditional: false,
        };
        
        graph.edges.push(edge);
        
        let json = graph.to_json().unwrap();
        let parsed = ImportsGraph::from_json(&json).unwrap();
        
        assert_eq!(parsed.edges.len(), 1);
        assert_eq!(parsed.edges[0].from_module, "test_module");
        assert_eq!(parsed.edges[0].names, vec!["function"]);
        assert!(matches!(parsed.edges[0].import_type, ImportType::From));
    }

    #[test]
    fn test_event_serialization() {
        let start_event = Event::Start {
            ts: Utc::now(),
            run_id: "test-run".to_string(),
            veri_version: "0.1.0".to_string(),
            python_version: "3.11.0".to_string(),
            platform: "linux-x86_64".to_string(),
            workers: 4,
            cache_key: "abc123".to_string(),
        };
        
        let json_line = start_event.to_jsonl_line().unwrap();
        let parsed = Event::from_jsonl_line(&json_line).unwrap();
        
        if let Event::Start { veri_version, workers, .. } = parsed {
            assert_eq!(veri_version, "0.1.0");
            assert_eq!(workers, 4);
        } else {
            panic!("Expected Start event");
        }
    }

    #[test]
    fn test_shards_manifest_serialization() {
        let mut manifest = ShardsManifest::new(4, ShardingStrategy::TimingBased);
        
        let shard = Shard {
            shard_id: 0,
            estimated_duration: 10.5,
            test_count: 5,
            tests: vec![
                ShardTest {
                    nodeid: "test_a.py::test_1".to_string(),
                    estimated_duration: 2.1,
                    priority: 0,
                    markers: vec!["unit".to_string()],
                }
            ],
        };
        
        manifest.shards.push(shard);
        manifest.estimated_duration = 42.0;
        
        let json = manifest.to_json().unwrap();
        let parsed = ShardsManifest::from_json(&json).unwrap();
        
        assert_eq!(parsed.total_shards, 4);
        assert_eq!(parsed.format_version, "veri-shards@1");
        assert!(matches!(parsed.strategy, ShardingStrategy::TimingBased));
        assert_eq!(parsed.shards.len(), 1);
        assert_eq!(parsed.shards[0].test_count, 5);
    }

    #[test]
    fn test_test_timings_serialization() {
        let mut timings = TestTimings::new();
        
        let test_timing = TestTiming {
            nodeid: "test_example.py::test_function".to_string(),
            setup_duration: 0.1,
            call_duration: 0.5,
            teardown_duration: 0.1,
            total_duration: 0.7,
            outcome: TestOutcome::Passed,
            worker_id: Some("worker-0".to_string()),
        };
        
        let timing_run = TimingRun {
            run_id: "run-123".to_string(),
            started_at: Utc::now(),
            finished_at: Utc::now(),
            workers: 4,
            test_timings: {
                let mut map = std::collections::HashMap::new();
                map.insert("test_example.py::test_function".to_string(), test_timing);
                map
            },
        };
        
        timings.runs.push(timing_run);
        
        let json = timings.to_json().unwrap();
        let parsed = TestTimings::from_json(&json).unwrap();
        
        assert_eq!(parsed.runs.len(), 1);
        assert_eq!(parsed.runs[0].workers, 4);
        assert_eq!(parsed.runs[0].test_timings.len(), 1);
    }
}