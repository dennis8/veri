use anyhow::{Context, Result};
use log::{debug, info, warn};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use crate::import_graph::ImportGraphBuilder;
use crate::planner::TestPlanner;
use crate::python_worker::{PythonWorker, TestRunOptions};

#[derive(Debug, Clone)]
pub struct WatchConfig {
    /// Debounce delay for file changes
    pub debounce_delay: Duration,
    /// Maximum time to wait for additional changes before triggering
    pub max_wait_time: Duration,
    /// Directories to ignore (relative to workspace root)
    pub ignore_dirs: Vec<String>,
    /// File patterns to ignore
    pub ignore_patterns: Vec<String>,
    /// Use git ignore rules
    pub respect_gitignore: bool,
    /// Enable terminal UI
    pub enable_tui: bool,
    /// Verbose output
    pub verbose: bool,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            debounce_delay: Duration::from_millis(150),
            max_wait_time: Duration::from_millis(500),
            ignore_dirs: vec![
                ".git".to_string(),
                ".veri".to_string(),
                "__pycache__".to_string(),
                ".pytest_cache".to_string(),
                "node_modules".to_string(),
                ".venv".to_string(),
                "venv".to_string(),
                ".env".to_string(),
                "env".to_string(),
                "htmlcov".to_string(),
                "reports".to_string(),
                "target".to_string(),
                "build".to_string(),
                "dist".to_string(),
                ".mypy_cache".to_string(),
                ".ruff_cache".to_string(),
            ],
            ignore_patterns: vec![
                "*.pyc".to_string(),
                "*.pyo".to_string(),
                "*.pyd".to_string(),
                "*.so".to_string(),
                "*.egg-info".to_string(),
                "*.coverage".to_string(),
                "*.log".to_string(),
                "*.tmp".to_string(),
                "*.swp".to_string(),
                "*.swo".to_string(),
                "*~".to_string(),
                ".DS_Store".to_string(),
                "Thumbs.db".to_string(),
            ],
            respect_gitignore: true,
            enable_tui: true,
            verbose: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileChangeEvent {
    pub path: PathBuf,
    pub timestamp: Instant,
}

#[derive(Debug)]
pub struct WatchSession {
    config: WatchConfig,
    work_dir: PathBuf,
    cache_dir: PathBuf,
    watcher: Option<RecommendedWatcher>,
    event_receiver: Option<Receiver<notify::Result<Event>>>,
    gitignore: Option<ignore::gitignore::Gitignore>,
    tui: Option<WatchTui>,
}

impl WatchSession {
    pub fn new(work_dir: PathBuf, cache_dir: PathBuf, config: WatchConfig) -> Result<Self> {
        let gitignore = if config.respect_gitignore {
            Self::load_gitignore(&work_dir)?
        } else {
            None
        };

        let tui = if config.enable_tui {
            Some(WatchTui::new()?)
        } else {
            None
        };

        Ok(Self {
            config,
            work_dir,
            cache_dir,
            watcher: None,
            event_receiver: None,
            gitignore,
            tui,
        })
    }

    fn load_gitignore(work_dir: &Path) -> Result<Option<ignore::gitignore::Gitignore>> {
        let gitignore_path = work_dir.join(".gitignore");
        if !gitignore_path.exists() {
            return Ok(None);
        }

        let mut builder = ignore::gitignore::GitignoreBuilder::new(work_dir);
        builder.add(&gitignore_path);

        match builder.build() {
            Ok(gitignore) => Ok(Some(gitignore)),
            Err(e) => {
                warn!("Failed to load .gitignore: {}", e);
                Ok(None)
            }
        }
    }

    pub fn start(&mut self) -> Result<()> {
        info!("Starting watch mode on {}", self.work_dir.display());

        let (tx, rx) = mpsc::channel();
        let mut watcher =
            notify::recommended_watcher(tx).context("Failed to create file system watcher")?;

        watcher
            .watch(&self.work_dir, RecursiveMode::Recursive)
            .context("Failed to start watching directory")?;

        self.watcher = Some(watcher);
        self.event_receiver = Some(rx);

        if let Some(tui) = &mut self.tui {
            tui.start()?;
            tui.show_initial_status(&self.work_dir)?;
        } else {
            println!(
                "👀 Watching for changes in {} (press Ctrl+C to stop)",
                self.work_dir.display()
            );
        }

        Ok(())
    }

    pub fn run(&mut self, test_run_options: TestRunOptions) -> Result<()> {
        let receiver = self
            .event_receiver
            .take()
            .ok_or_else(|| anyhow::anyhow!("Watch session not started"))?;

        let mut debouncer =
            FileChangeDebouncer::new(self.config.debounce_delay, self.config.max_wait_time);
        let mut last_run = Instant::now();

        loop {
            // Check for file system events
            match receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(Ok(event)) => {
                    if let Some(change_event) = self.process_fs_event(event)? {
                        debouncer.add_change(change_event);
                    }
                }
                Ok(Err(e)) => {
                    warn!("File watcher error: {}", e);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // No events - continue with debounce check
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break;
                }
            }

            // Check if we should trigger a test run
            if let Some(changed_files) = debouncer.should_trigger() {
                let now = Instant::now();

                // Avoid triggering too frequently
                if now.duration_since(last_run) < Duration::from_millis(100) {
                    debouncer.reset(); // Reset to avoid rapid fire
                    continue;
                }

                last_run = now;

                if self.config.verbose {
                    info!(
                        "Triggering test run for {} changed files",
                        changed_files.len()
                    );
                    for file in &changed_files {
                        debug!("  Changed: {}", file.display());
                    }
                }

                if let Some(tui) = &mut self.tui {
                    tui.show_run_starting(&changed_files)?;
                }

                // Run tests for the changed files
                match self.run_impacted_tests(&changed_files, &test_run_options) {
                    Ok(result) => {
                        if let Some(tui) = &mut self.tui {
                            tui.show_run_completed(&result)?;
                        } else {
                            self.print_run_summary(&result);
                        }
                    }
                    Err(e) => {
                        if let Some(tui) = &mut self.tui {
                            tui.show_error(&e)?;
                        } else {
                            eprintln!("❌ Test run failed: {}", e);
                        }
                    }
                }
            }

            // Update TUI if enabled
            if let Some(tui) = &mut self.tui {
                tui.update()?;
            }

            // Check for user input (Ctrl+C handling is done by signal handlers)
            std::thread::sleep(Duration::from_millis(10));
        }

        Ok(())
    }

    fn process_fs_event(&self, event: Event) -> Result<Option<FileChangeEvent>> {
        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
                for path in event.paths {
                    if self.should_process_file(&path) {
                        return Ok(Some(FileChangeEvent {
                            path,
                            timestamp: Instant::now(),
                        }));
                    }
                }
            }
            _ => {
                // Ignore other event types
            }
        }
        Ok(None)
    }

    fn should_process_file(&self, path: &Path) -> bool {
        // Convert to relative path from work_dir
        let relative_path = match path.strip_prefix(&self.work_dir) {
            Ok(rel) => rel,
            Err(_) => return false, // Path outside work directory
        };

        // Check if it's a Python file or conftest.py
        let is_python_file = path.extension().is_some_and(|ext| ext == "py")
            || path.file_name().is_some_and(|name| name == "conftest.py");

        if !is_python_file {
            return false;
        }

        // Check ignore directories
        for component in relative_path.components() {
            if let std::path::Component::Normal(name) = component {
                let name_str = name.to_string_lossy();
                if self
                    .config
                    .ignore_dirs
                    .iter()
                    .any(|ignore| *ignore == name_str)
                {
                    return false;
                }
            }
        }

        // Check ignore patterns
        let path_str = relative_path.to_string_lossy();
        for pattern in &self.config.ignore_patterns {
            if glob_match(pattern, &path_str) {
                return false;
            }
        }

        // Check gitignore
        if let Some(gitignore) = &self.gitignore {
            if gitignore.matched(relative_path, false).is_ignore() {
                return false;
            }
        }

        true
    }

    fn run_impacted_tests(
        &self,
        changed_files: &[PathBuf],
        test_run_options: &TestRunOptions,
    ) -> Result<TestRunResult> {
        let start_time = Instant::now();

        // Initialize components
        let worker = PythonWorker::new(&self.work_dir, &self.cache_dir);
        let mut graph_builder = ImportGraphBuilder::new(&self.work_dir, &self.cache_dir);

        // Load or build import graphs
        let (imports_graph, revdeps_graph, module_map) = match graph_builder.load_cached_graphs()? {
            Some(graphs) => graphs,
            None => {
                // Build graphs if not cached
                if self.config.verbose {
                    println!("🔍 Building import graph for impact analysis...");
                }
                graph_builder.build_graphs()?
            }
        };

        // Load test index
        let tests_index = match worker.collect_tests(&[], &[]) {
            Ok(index) => index,
            Err(e) => {
                warn!("Failed to collect tests in watch mode: {}", e);
                // Return empty result - no tests to run
                return Ok(TestRunResult {
                    duration: start_time.elapsed(),
                    tests_run: 0,
                    failures: 0,
                    exit_code: 0,
                    changed_files: changed_files
                        .iter()
                        .filter_map(|path| {
                            path.strip_prefix(&self.work_dir)
                                .ok()
                                .map(|rel| rel.to_string_lossy().replace('\\', "/"))
                        })
                        .collect(),
                    selected_tests: Vec::new(),
                    was_broadened: false,
                    broaden_reason: Some(format!("Test collection failed: {}", e)),
                });
            }
        };

        // Convert changed files to relative string paths
        let changed_file_strs: Vec<String> = changed_files
            .iter()
            .filter_map(|path| {
                path.strip_prefix(&self.work_dir)
                    .ok()
                    .map(|rel| rel.to_string_lossy().replace('\\', "/"))
            })
            .collect();

        // Plan test selection
        let planner = TestPlanner::new(&self.work_dir, &self.cache_dir);
        let selection = match planner.plan_test_selection(
            &changed_file_strs,
            &tests_index,
            &revdeps_graph,
            &module_map,
            &imports_graph,
        ) {
            Ok(sel) => sel,
            Err(e) => {
                warn!("Impact analysis failed: {}", e);
                // Fallback: run all tests
                let all_tests: Vec<String> =
                    tests_index.tests.iter().map(|t| t.nodeid.clone()).collect();

                return Ok(TestRunResult {
                    duration: start_time.elapsed(),
                    tests_run: all_tests.len(),
                    failures: 0,
                    exit_code: 0,
                    changed_files: changed_file_strs,
                    selected_tests: all_tests,
                    was_broadened: true,
                    broaden_reason: Some(format!(
                        "Impact analysis failed, running all tests: {}",
                        e
                    )),
                });
            }
        };

        if selection.selected_nodeids.is_empty() {
            return Ok(TestRunResult {
                duration: start_time.elapsed(),
                tests_run: 0,
                failures: 0,
                exit_code: 0,
                changed_files: changed_file_strs,
                selected_tests: Vec::new(),
                was_broadened: false,
                broaden_reason: None,
            });
        }

        // Run the selected tests
        let (exit_code, _stdout, _stderr) = match worker.run_tests(&selection.selected_nodeids, test_run_options) {
            Ok(exec) => (exec.exit_code, exec.stdout, exec.stderr),
            Err(e) => {
                warn!("Test execution failed: {}", e);
                (-1, String::new(), String::new()) // Indicate execution failure
            }
        };

        Ok(TestRunResult {
            duration: start_time.elapsed(),
            tests_run: selection.selected_nodeids.len(),
            failures: if exit_code == 0 { 0 } else { 1 }, // Simplified for now
            exit_code,
            changed_files: changed_file_strs,
            selected_tests: selection.selected_nodeids,
            was_broadened: selection.should_broaden,
            broaden_reason: selection.broaden_reason,
        })
    }

    fn print_run_summary(&self, result: &TestRunResult) {
        let status_icon = if result.exit_code == 0 { "✅" } else { "❌" };
        let duration_ms = result.duration.as_millis();

        println!(
            "{} {} tests completed in {}ms",
            status_icon, result.tests_run, duration_ms
        );

        if result.was_broadened {
            if let Some(reason) = &result.broaden_reason {
                println!("⚠️  Selection broadened: {}", reason);
            }
        }

        if self.config.verbose && !result.changed_files.is_empty() {
            println!("📁 Changed files:");
            for file in &result.changed_files {
                println!("   {}", file);
            }
        }

        println!("👀 Watching for changes...");
    }
}

#[derive(Debug)]
struct FileChangeDebouncer {
    changes: Vec<FileChangeEvent>,
    debounce_delay: Duration,
    max_wait_time: Duration,
    first_change_time: Option<Instant>,
}

impl FileChangeDebouncer {
    fn new(debounce_delay: Duration, max_wait_time: Duration) -> Self {
        Self {
            changes: Vec::new(),
            debounce_delay,
            max_wait_time,
            first_change_time: None,
        }
    }

    fn add_change(&mut self, change: FileChangeEvent) {
        if self.first_change_time.is_none() {
            self.first_change_time = Some(change.timestamp);
        }

        // Remove any previous change for the same file
        self.changes.retain(|existing| existing.path != change.path);
        self.changes.push(change);
    }

    fn should_trigger(&mut self) -> Option<Vec<PathBuf>> {
        if self.changes.is_empty() {
            return None;
        }

        let now = Instant::now();
        let first_change = self.first_change_time?;

        // Find the most recent change
        let last_change_time = self.changes.iter().map(|change| change.timestamp).max()?;

        let time_since_last = now.duration_since(last_change_time);
        let time_since_first = now.duration_since(first_change);

        // Trigger if debounce period has passed or max wait time exceeded
        if time_since_last >= self.debounce_delay || time_since_first >= self.max_wait_time {
            let files = self
                .changes
                .iter()
                .map(|change| change.path.clone())
                .collect();
            self.reset();
            Some(files)
        } else {
            None
        }
    }

    fn reset(&mut self) {
        self.changes.clear();
        self.first_change_time = None;
    }
}

#[derive(Debug)]
pub struct TestRunResult {
    pub duration: Duration,
    pub tests_run: usize,
    pub failures: usize,
    pub exit_code: i32,
    pub changed_files: Vec<String>,
    pub selected_tests: Vec<String>,
    pub was_broadened: bool,
    pub broaden_reason: Option<String>,
}

/// Simple TUI for watch mode
#[derive(Debug)]
struct WatchTui {
    // Placeholder - will be expanded later
}

impl WatchTui {
    fn new() -> Result<Self> {
        Ok(Self {})
    }

    fn start(&mut self) -> Result<()> {
        // Enable raw mode for better terminal control
        crossterm::terminal::enable_raw_mode()?;
        Ok(())
    }

    fn show_initial_status(&mut self, work_dir: &Path) -> Result<()> {
        println!("👀 veri watch mode");
        println!("📁 Monitoring: {}", work_dir.display());
        println!("⚡ Ready for changes (press Ctrl+C to stop)");
        println!();
        Ok(())
    }

    fn show_run_starting(&mut self, changed_files: &[PathBuf]) -> Result<()> {
        print!(
            "\r🔄 Running tests for {} changed file(s)... ",
            changed_files.len()
        );
        std::io::Write::flush(&mut std::io::stdout())?;
        Ok(())
    }

    fn show_run_completed(&mut self, result: &TestRunResult) -> Result<()> {
        let status_icon = if result.exit_code == 0 { "✅" } else { "❌" };
        let duration_ms = result.duration.as_millis();

        println!(
            "\r{} {} tests in {}ms",
            status_icon, result.tests_run, duration_ms
        );

        if result.was_broadened {
            println!("⚠️  Selection broadened");
        }

        println!("👀 Watching for changes...");
        Ok(())
    }

    fn show_error(&mut self, error: &anyhow::Error) -> Result<()> {
        println!("\r❌ Error: {}", error);
        println!("👀 Watching for changes...");
        Ok(())
    }

    fn update(&mut self) -> Result<()> {
        // Placeholder for TUI updates
        Ok(())
    }
}

impl Drop for WatchTui {
    fn drop(&mut self) {
        // Restore terminal state
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

/// Simple glob pattern matching
fn glob_match(pattern: &str, text: &str) -> bool {
    // Handle exact match first
    if pattern == text {
        return true;
    }

    // Handle patterns with * wildcard
    if pattern.contains('*') {
        if let Some(middle) = pattern
            .strip_prefix('*')
            .and_then(|rest| rest.strip_suffix('*'))
        {
            // *substring* pattern
            text.contains(middle)
        } else if let Some(suffix) = pattern.strip_prefix('*') {
            // *suffix pattern
            text.ends_with(suffix)
        } else if let Some(prefix) = pattern.strip_suffix('*') {
            // prefix* pattern
            text.starts_with(prefix)
        } else {
            // More complex pattern - for now, just do simple contains check
            let parts: Vec<&str> = pattern.split('*').collect();
            if parts.len() == 2 {
                text.starts_with(parts[0]) && text.ends_with(parts[1])
            } else {
                false // More complex patterns not supported yet
            }
        }
    } else {
        // No wildcards, must be exact match
        pattern == text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_debouncer() {
        let mut debouncer =
            FileChangeDebouncer::new(Duration::from_millis(100), Duration::from_millis(500));

        // Add a change
        let path = PathBuf::from("test.py");
        debouncer.add_change(FileChangeEvent {
            path: path.clone(),
            timestamp: Instant::now(),
        });

        // Should not trigger immediately
        assert!(debouncer.should_trigger().is_none());

        // Wait for debounce period
        std::thread::sleep(Duration::from_millis(150));

        // Should trigger now
        let result = debouncer.should_trigger();
        assert!(result.is_some());
        assert_eq!(result.unwrap(), vec![path]);

        // Should be reset after triggering
        assert!(debouncer.should_trigger().is_none());
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("*.py", "test.py"));
        assert!(glob_match("*.py", ".py"));
        assert!(!glob_match("*.py", "test.txt"));
        assert!(glob_match("test.py", "test.py"));
        assert!(!glob_match("test.py", "other.py"));
    }
}
