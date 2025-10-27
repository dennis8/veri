use crate::cli::{Cli, Commands, ExitCode};
use anyhow::{anyhow, Result};
use std::io::Read;
use std::time::Instant;
use veri_core::config::Config;
use veri_core::event_stream::{generate_run_id, CIReporter};
use veri_core::python_launcher::PythonRuntime;
use veri_core::python_worker::{PythonWorker, TestRunOptions};
use veri_core::sharder::{SharderConfig, TestSharder};

pub fn handle_subcommand(command: &Commands, config: &Config, cli_args: &Cli) -> Result<ExitCode> {
    let work_dir = std::env::current_dir()?;
    let cache_dir = work_dir.join(".veri").join("cache");
    let python_runtime_cfg = config.python();
    let python_runtime = PythonRuntime::from_config(&work_dir, &python_runtime_cfg);

    match command {
        Commands::Split { shards } => {
            eprintln!("🔀 Splitting tests into {} shards", shards);

            let sharder_config = SharderConfig {
                strategy: veri_core::schemas::ShardingStrategy::TimingBased,
                ..Default::default()
            };
            let sharder = TestSharder::with_config(&work_dir, &cache_dir, sharder_config);

            let manifest = sharder.split_tests(*shards, None)?;
            let stats = sharder.generate_stats_summary(&manifest);

            let manifest_json = serde_json::to_string_pretty(&manifest)?;
            println!("{}", manifest_json);

            eprintln!(
                "📊 Generated {} shards with {:.1}% load balance",
                stats.total_shards,
                stats.balance_ratio * 100.0
            );
            eprintln!(
                "⏱️  Total estimated duration: {:.1}s (avg per shard: {:.1}s)",
                stats.total_estimated_duration, stats.avg_shard_duration
            );

            if cli_args.verbose > 0 {
                eprintln!("{}", sharder.format_explain(&manifest, &stats));
            }

            Ok(ExitCode::Success)
        }
        Commands::Shard { shard_id, manifest } => {
            println!("🎯 Running shard {} of CI execution", shard_id);

            let manifest_data = if let Some(manifest_path) = manifest {
                std::fs::read_to_string(manifest_path)?
            } else {
                let mut buffer = String::new();
                std::io::stdin().read_to_string(&mut buffer)?;
                buffer
            };

            let manifest: veri_core::schemas::ShardsManifest =
                serde_json::from_str(&manifest_data)?;

            let sharder = TestSharder::new(&work_dir, &cache_dir);
            sharder.validate_manifest(&manifest)?;

            let shard = sharder
                .get_shard(&manifest, *shard_id)?
                .ok_or_else(|| anyhow!("Shard {} not found in manifest", shard_id))?;

            println!(
                "📋 Shard {}: {} tests, estimated {:.1}s",
                shard.shard_id, shard.test_count, shard.estimated_duration
            );

            let nodeids = sharder.extract_nodeids(shard);

            if nodeids.is_empty() {
                println!("✅ No tests to run in this shard");
                return Ok(ExitCode::Success);
            }

            let run_id = generate_run_id();
            let mut ci_reporter = CIReporter::with_shard(run_id.clone(), *shard_id);

            if let Some(jsonl_path) = &config.jsonl {
                ci_reporter.init_jsonl(jsonl_path)?;
                if let Some(stream) = ci_reporter.event_stream() {
                    stream.emit_start(
                        manifest.shards.iter().map(|s| s.test_count).sum(),
                        shard.test_count,
                        1,
                        "shard",
                    )?;
                    stream.emit_plan(nodeids.clone(), None, shard.estimated_duration)?;
                }
            }

            if let Some(junit_path) = &config.junit_xml {
                ci_reporter.init_junit(junit_path)?;
            }

            println!("🚀 Executing {} tests...", nodeids.len());
            let start_time = Instant::now();
            let worker = PythonWorker::from_runtime(&work_dir, &cache_dir, &python_runtime);
            let tests_index = worker.collect_tests(&[], &[])?;
            let run_options = TestRunOptions {
                verbose: cli_args.verbose > 0,
                quiet: cli_args.quiet,
                no_capture: cli_args.no_capture,
                exitfirst: cli_args.exitfirst,
                maxfail: cli_args.maxfail,
                junit_xml: config.junit_xml.clone(),
                workers: Some("1".to_string()),
                ignore: cli_args.ignore.clone(),
                coverage: config.cov.unwrap_or(false),
                coverage_xml: config.cov.unwrap_or(false) || config.cov_merge_full.unwrap_or(false),
                coverage_html: false,
                coverage_source_dirs: vec!["src".to_string()],
                coverage_omit: vec![
                    "*/tests/*".to_string(),
                    "*/test_*".to_string(),
                    "*/__pycache__/*".to_string(),
                    "*/venv/*".to_string(),
                    "*/.venv/*".to_string(),
                ],
            };
            let exec_config = crate::orchestrator::ParallelExecutionConfig {
                work_dir: &work_dir,
                cache_dir: &cache_dir,
                verbose: cli_args.verbose > 0,
                config,
                python_runtime: python_runtime.clone(),
            };
            let exit = crate::orchestrator::execute_tests_parallel(
                &nodeids,
                &tests_index,
                1,
                &run_options,
                &exec_config,
            )?;
            let duration = start_time.elapsed().as_secs_f64();

            if let Some(stream) = ci_reporter.event_stream() {
                // TODO: Get actual test statistics from execute_tests_parallel return value
                // For now, we approximate based on exit code
                let (passed, failed) = if exit == ExitCode::Success {
                    (nodeids.len() as u32, 0)
                } else {
                    // Some tests failed, but we don't know exact counts without refactoring
                    (0, nodeids.len() as u32)
                };
                stream.emit_summary(
                    duration,
                    nodeids.len() as u32,
                    passed,
                    failed,
                    0,
                    0,
                    exit as i32,
                )?;
            }

            ci_reporter.finalize()?;

            println!("✅ Shard {} completed in {:.1}s", shard_id, duration);

            Ok(exit)
        }
    }
}
