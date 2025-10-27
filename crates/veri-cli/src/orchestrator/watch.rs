use crate::cli::{Cli, ExitCode};
use anyhow::Result;
use std::path::Path;
use veri_core::diagnostics::{DiagnosticReporter, VeriDiagnostic};
use veri_core::import_graph::ImportGraphBuilder;
use veri_core::python_worker::{PythonWorker, TestRunOptions};
use veri_core::watch::{WatchConfig, WatchSession};

pub trait WatchAdapter: Send {
    fn run(&self, cli: &Cli, work_dir: &Path, cache_dir: &Path) -> Result<ExitCode>;
}

#[derive(Default)]
pub struct DefaultWatchAdapter;

impl WatchAdapter for DefaultWatchAdapter {
    fn run(&self, cli: &Cli, work_dir: &Path, cache_dir: &Path) -> Result<ExitCode> {
        run_watch_mode(cli, work_dir, cache_dir)
    }
}

fn run_watch_mode(cli: &Cli, work_dir: &Path, cache_dir: &Path) -> Result<ExitCode> {
    println!("👀 Starting watch mode...");

    let watch_config = WatchConfig {
        debounce_delay: std::time::Duration::from_millis(150),
        max_wait_time: std::time::Duration::from_millis(500),
        respect_gitignore: true,
        enable_tui: !cli.quiet,
        verbose: cli.verbose > 0,
        ..Default::default()
    };

    let run_options = TestRunOptions {
        verbose: cli.verbose > 0,
        quiet: cli.quiet,
        no_capture: cli.no_capture,
        exitfirst: cli.exitfirst,
        maxfail: cli.maxfail,
        junit_xml: cli.junit_xml.clone(),
        workers: Some("1".to_string()),
        ignore: cli.ignore.clone(),
        coverage: cli.cov,
        coverage_xml: cli.cov || cli.cov_merge_full,
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

    let worker = PythonWorker::new(work_dir, cache_dir);
    if !worker.has_valid_cache() {
        println!("📋 Initial collection required...");
        match worker.collect_tests(&[], &[]) {
            Ok(_tests_index) => {
                println!("✅ Initial collection completed");
            }
            Err(e) => {
                println!("⚠️  Initial collection failed: {}", e);
                println!("   Watch mode will continue but may have limited functionality");
            }
        }

        println!("🔍 Building import graph...");
        let mut graph_builder = ImportGraphBuilder::new(work_dir, cache_dir);
        let mut diagnostics = DiagnosticReporter::new(false);

        match graph_builder.build_graphs() {
            Ok((imports_graph, _revdeps_graph, _module_map)) => {
                println!(
                    "✅ Built import graph with {} edges",
                    imports_graph.edges.len()
                );

                if !imports_graph.unresolved_imports.is_empty() {
                    let missing_files: Vec<String> = imports_graph
                        .unresolved_imports
                        .iter()
                        .map(|u| format!("{} (from {})", u.import_name, u.from_module))
                        .collect();

                    diagnostics.add(VeriDiagnostic::ImportGraphBuildFailed {
                        error_count: imports_graph.unresolved_imports.len(),
                        syntax_errors: Vec::new(),
                        missing_files,
                    });
                }
            }
            Err(e) => {
                diagnostics.add(VeriDiagnostic::ImportGraphBuildFailed {
                    error_count: 1,
                    syntax_errors: vec![e.to_string()],
                    missing_files: Vec::new(),
                });

                println!("⚠️  Failed to build import graph: {}", e);
                println!("   Watch mode will run all tests when files change");
            }
        }

        diagnostics.report_all()?;
    } else {
        println!("✅ Using cached test collection and import graph");
    }

    let mut watch_session = WatchSession::new(
        work_dir.to_path_buf(),
        cache_dir.to_path_buf(),
        watch_config,
    )?;

    watch_session.start()?;

    setup_signal_handlers()?;

    watch_session.run(run_options)?;

    Ok(ExitCode::Success)
}

fn setup_signal_handlers() -> Result<()> {
    ctrlc::set_handler(move || {
        println!("\n🛑 Stopping watch mode...");
        std::process::exit(0);
    })?;

    Ok(())
}
