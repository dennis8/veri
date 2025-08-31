//! Worker process pool for parallel test execution
//!
//! This module provides a managed pool of Python worker processes that can
//! execute tests in parallel while maintaining warm interpreter state for
//! improved performance.

use anyhow::{anyhow, Context, Result};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crate::python_worker::TestRunOptions;
use crate::scheduler::TestBatch;

/// Configuration for the worker pool
#[derive(Debug, Clone)]
pub struct WorkerPoolConfig {
    /// Number of worker processes to maintain
    pub worker_count: usize,
    /// Timeout for worker startup
    pub startup_timeout: Duration,
    /// Timeout for individual test execution
    pub execution_timeout: Duration,
    /// Heartbeat interval to ping workers
    pub heartbeat_interval: Duration,
    /// Maximum idle time before worker recycling
    pub max_idle_time: Duration,
    /// Enable worker process recycling
    pub enable_recycling: bool,
    /// Working directory for workers
    pub work_dir: PathBuf,
    /// Cache directory for workers
    pub cache_dir: PathBuf,
}

impl Default for WorkerPoolConfig {
    fn default() -> Self {
        Self {
            worker_count: num_cpus::get().max(1),
            startup_timeout: Duration::from_secs(30),
            execution_timeout: Duration::from_secs(300), // 5 minutes
            heartbeat_interval: Duration::from_secs(10),
            max_idle_time: Duration::from_secs(600),     // 10 minutes
            enable_recycling: true,
            work_dir: std::env::current_dir().unwrap_or_default(),
            cache_dir: std::env::current_dir()
                .unwrap_or_default()
                .join(".veri")
                .join("cache"),
        }
    }
}

/// Message types for worker communication
#[derive(Debug, Clone)]
pub enum WorkerMessage {
    ExecuteTests {
        batch_id: String,
        nodeids: Vec<String>,
        options: TestRunOptions,
    },
    Shutdown,
    HealthCheck,
}

/// Response types from workers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkerResponse {
    TestResults {
        batch_id: String,
        exit_code: i32,
        stdout: String,
        stderr: String,
        duration_ms: u64,
        per_test: Option<Vec<PerTestResult>>,
    },
    HealthOk,
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerTestResult {
    pub nodeid: String,
    pub outcome: String,
    pub duration_ms: u64,
}

/// Worker process state
#[derive(Debug)]
#[allow(dead_code)]
enum WorkerState {
    Starting,
    Idle,
    Busy(String),   // batch_id
    Failed(String), // error message
    Shutdown,
}

/// Individual worker process wrapper
struct WorkerProcess {
    id: usize,
    process: Option<Child>,
    state: WorkerState,
    started_at: Instant,
    last_activity: Instant,
    sender: Option<Sender<WorkerMessage>>, // to writer thread
}

impl WorkerProcess {
    fn new(id: usize) -> Self {
        let now = Instant::now();
        Self {
            id,
            process: None,
            state: WorkerState::Starting,
            started_at: now,
            last_activity: now,
            sender: None,
        }
    }

    fn is_available(&self) -> bool {
        matches!(self.state, WorkerState::Idle)
    }

    fn is_failed(&self) -> bool {
        matches!(self.state, WorkerState::Failed(_))
    }

    fn should_recycle(&self, max_idle_time: Duration) -> bool {
        matches!(self.state, WorkerState::Idle) && self.last_activity.elapsed() > max_idle_time
    }
}

/// Managed pool of worker processes for parallel test execution
pub struct WorkerPool {
    config: WorkerPoolConfig,
    workers: Vec<WorkerProcess>,
    task_queue: VecDeque<PendingTask>,
    active_tasks: HashMap<String, TaskContext>,
    shutdown_requested: bool,
    evt_tx: Sender<WorkerEvent>,
    evt_rx: Receiver<WorkerEvent>,
}

/// Task waiting to be executed
#[derive(Debug)]
#[allow(dead_code)]
struct PendingTask {
    batch_id: String,
    batch: TestBatch,
    options: TestRunOptions,
    submitted_at: Instant,
}

/// Context for an active task
#[derive(Debug)]
#[allow(dead_code)]
struct TaskContext {
    worker_id: usize,
    started_at: Instant,
    batch: TestBatch,
}

/// Results from executing a batch of tests
#[derive(Debug)]
pub struct BatchResult {
    pub batch_id: String,
    pub worker_id: usize,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
    pub nodeids: Vec<String>,
    pub per_test: Vec<PerTestResult>,
}

#[derive(Debug)]
enum WorkerEvent {
    HelloOk { worker_id: usize },
    HealthOk { worker_id: usize },
    TestResults { worker_id: usize, response: WorkerResponse },
}

impl WorkerPool {
    /// Create a new worker pool
    pub fn new(config: WorkerPoolConfig) -> Self {
        let (evt_tx, evt_rx) = mpsc::channel();
        Self {
            config,
            workers: Vec::new(),
            task_queue: VecDeque::new(),
            active_tasks: HashMap::new(),
            shutdown_requested: false,
            evt_tx,
            evt_rx,
        }
    }

    /// Set up the worker script in the cache directory
    fn setup_worker_script(&self) -> Result<()> {
        let worker_script_dest = self.config.cache_dir.join("veri_worker.py");

        // Find the worker script in the source tree
        // Try common locations relative to the working directory
        let potential_paths = [
            self.config
                .work_dir
                .join("py_worker")
                .join("veri_worker.py"),
            self.config.work_dir.join("veri_worker.py"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("py_worker")
                .join("veri_worker.py"),
        ];

        let mut worker_script_src = None;
        for path in &potential_paths {
            if path.exists() {
                worker_script_src = Some(path.clone());
                break;
            }
        }

        let worker_script_src =
            worker_script_src.ok_or_else(|| anyhow!("Could not find veri_worker.py script"))?;

        debug!(
            "Copying worker script from {} to {}",
            worker_script_src.display(),
            worker_script_dest.display()
        );

        std::fs::copy(&worker_script_src, &worker_script_dest).with_context(|| {
            format!(
                "Failed to copy worker script from {} to {}",
                worker_script_src.display(),
                worker_script_dest.display()
            )
        })?;

        Ok(())
    }

    /// Initialize the worker pool and start worker processes
    pub fn start(&mut self) -> Result<()> {
        info!(
            "Starting worker pool with {} workers",
            self.config.worker_count
        );

        // Set up worker script
        self.setup_worker_script()?;

        // Initialize worker processes
        for i in 0..self.config.worker_count {
            let mut worker = WorkerProcess::new(i);
            self.start_worker_process(&mut worker)?;
            self.workers.push(worker);
        }

        info!("Worker pool started successfully");
        Ok(())
    }

    /// Submit a batch of tests for execution
    pub fn submit_batch(
        &mut self,
        batch_id: String,
        batch: TestBatch,
        options: TestRunOptions,
    ) -> Result<()> {
        if self.shutdown_requested {
            return Err(anyhow!("Worker pool is shutting down"));
        }

        let task = PendingTask {
            batch_id: batch_id.clone(),
            batch,
            options,
            submitted_at: Instant::now(),
        };

        debug!(
            "Submitting batch {} with {} tests",
            batch_id,
            task.batch.nodeids.len()
        );
        self.task_queue.push_back(task);

        // Try to assign to an available worker immediately
        self.process_queue()?;

        Ok(())
    }

    /// Process the task queue and assign work to available workers
    fn process_queue(&mut self) -> Result<()> {
        while let Some(_task) = self.task_queue.front() {
            // Find an available worker
            let available_worker_id = self
                .workers
                .iter()
                .enumerate()
                .find(|(_, w)| w.is_available())
                .map(|(i, _)| i);

            if let Some(worker_id) = available_worker_id {
                let task = self.task_queue.pop_front().unwrap();
                self.assign_task_to_worker(worker_id, task)?;
            } else {
                // No available workers, stop processing
                break;
            }
        }

        Ok(())
    }

    /// Assign a task to a specific worker
    fn assign_task_to_worker(&mut self, worker_id: usize, task: PendingTask) -> Result<()> {
        let worker = &mut self.workers[worker_id];

        debug!("Assigning batch {} to worker {}", task.batch_id, worker_id);

        // Send work to the worker
        if let Some(sender) = &worker.sender {
            let message = WorkerMessage::ExecuteTests {
                batch_id: task.batch_id.clone(),
                nodeids: task.batch.nodeids.clone(),
                options: task.options.clone(),
            };

            sender
                .send(message)
                .map_err(|e| anyhow!("Failed to send task to worker {}: {}", worker_id, e))?;
        } else {
            return Err(anyhow!("Worker {} is not ready", worker_id));
        }

        // Update worker state
        worker.state = WorkerState::Busy(task.batch_id.clone());
        worker.last_activity = Instant::now();

        // Track active task
        self.active_tasks.insert(
            task.batch_id.clone(),
            TaskContext {
                worker_id,
                started_at: Instant::now(),
                batch: task.batch,
            },
        );

        Ok(())
    }

    /// Poll for completed tasks (non-blocking)
    pub fn poll_results(&mut self) -> Result<Vec<BatchResult>> {
        let mut out = Vec::new();

        // Drain worker events
        for evt in self.evt_rx.try_iter() {
            match evt {
                WorkerEvent::TestResults { worker_id, response } => {
                    if let WorkerResponse::TestResults {
                        batch_id,
                        exit_code,
                        ref stdout,
                        ref stderr,
                        duration_ms,
                        ..
                    } = &response
                    {
                        if let Some(ctx) = self.active_tasks.remove(batch_id) {
                            let br = BatchResult {
                                batch_id: batch_id.clone(),
                                worker_id,
                                exit_code: *exit_code,
                                stdout: stdout.to_string(),
                                stderr: stderr.to_string(),
                                duration: Duration::from_millis(*duration_ms),
                                nodeids: ctx.batch.nodeids.clone(),
                                per_test: if let WorkerResponse::TestResults { per_test, .. } = response { per_test.unwrap_or_default() } else { Vec::new() },
                            };
                            out.push(br);
                            if let Some(w) = self.workers.get_mut(worker_id) {
                                w.state = WorkerState::Idle;
                                w.last_activity = Instant::now();
                            }
                        }
                    }
                }
                WorkerEvent::HelloOk { worker_id } => {
                    if let Some(w) = self.workers.get_mut(worker_id) {
                        w.last_activity = Instant::now();
                        if matches!(w.state, WorkerState::Starting) {
                            w.state = WorkerState::Idle;
                        }
                    }
                }
                WorkerEvent::HealthOk { worker_id } => {
                    if let Some(w) = self.workers.get_mut(worker_id) {
                        w.last_activity = Instant::now();
                    }
                }
            }
        }

        // Health + queue processing
        // Update liveness on HealthOk events already processed above
        self.check_worker_health()?;
        self.recycle_idle_workers()?;
        self.process_queue()?;

        Ok(out)
    }

    /// Wait for all active tasks to complete
    pub fn wait_for_completion(&mut self, timeout: Option<Duration>) -> Result<Vec<BatchResult>> {
        let start_time = Instant::now();
        let mut all_results = Vec::new();

        while !self.active_tasks.is_empty() {
            // Check timeout
            if let Some(timeout) = timeout {
                if start_time.elapsed() > timeout {
                    return Err(anyhow!("Timeout waiting for worker completion"));
                }
            }

            // Execution timeout handling
            let mut timed_out_batches = Vec::new();
            for (batch_id, ctx) in self.active_tasks.iter() {
                if ctx.started_at.elapsed() > self.config.execution_timeout {
                    timed_out_batches.push((batch_id.clone(), ctx.worker_id));
                }
            }
            for (batch_id, worker_id) in timed_out_batches {
                warn!(
                    "Batch {} timed out on worker {} (>{:?})",
                    batch_id, worker_id, self.config.execution_timeout
                );
                if let Some(worker) = self.workers.get_mut(worker_id) {
                    if let Some(proc) = &mut worker.process {
                        let _ = proc.kill();
                        let _ = proc.wait();
                        worker.process = None;
                    }
                    worker.state = WorkerState::Failed("Execution timeout".to_string());
                }
                if let Some(ctx) = self.active_tasks.remove(&batch_id) {
                    let br = BatchResult {
                        batch_id: batch_id.clone(),
                        worker_id,
                        exit_code: 2,
                        stdout: String::new(),
                        stderr: String::from("Timed out"),
                        duration: ctx.started_at.elapsed(),
                        nodeids: ctx.batch.nodeids.clone(),
                        per_test: Vec::new(),
                    };
                    all_results.push(br);
                }
            }

            // Poll for results
            let mut results = self.poll_results()?;
            all_results.append(&mut results);

            // Brief sleep to avoid busy polling
            thread::sleep(Duration::from_millis(50));
        }

        Ok(all_results)
    }

    /// Gracefully shutdown the worker pool
    pub fn shutdown(&mut self) -> Result<()> {
        info!("Shutting down worker pool");
        self.shutdown_requested = true;

        // Send shutdown messages to all workers
        for worker in &mut self.workers {
            if let Some(sender) = &worker.sender {
                let _ = sender.send(WorkerMessage::Shutdown);
            }
            worker.state = WorkerState::Shutdown;
        }

        // Wait for processes to exit or kill them
        let shutdown_timeout = Duration::from_secs(10);
        let shutdown_start = Instant::now();

        for worker in &mut self.workers {
            if let Some(process) = &mut worker.process {
                let remaining_time = shutdown_timeout.saturating_sub(shutdown_start.elapsed());

                if remaining_time > Duration::ZERO {
                    // Try graceful shutdown first
                    if let Ok(Some(_)) = process.try_wait() {
                        continue; // Already exited
                    }

                    // Wait a bit for graceful shutdown
                    thread::sleep(Duration::from_millis(100));

                    if let Ok(Some(_)) = process.try_wait() {
                        continue; // Exited gracefully
                    }
                }

                // Force kill if still running
                warn!("Force killing worker process {}", worker.id);
                let _ = process.kill();
                let _ = process.wait();
            }
        }

        info!("Worker pool shutdown complete");
        Ok(())
    }

    /// Start an individual worker process
    fn start_worker_process(&mut self, worker: &mut WorkerProcess) -> Result<()> {
        debug!("Starting worker process {}", worker.id);

        // Prefer launching via `uv run --project <py_worker>` to ensure pytest deps are available
        let mut cmd;
        if let Some(py_worker_path) = Self::find_py_worker_path(&self.config.work_dir) {
            cmd = Command::new("uv");
            cmd.arg("run")
                .arg("--project")
                .arg(py_worker_path)
                .arg("python")
                .arg("-m")
                .arg("veri_worker");
        } else {
            // Fallback to system python and cached script
            let python_executable = self.get_python_executable()?;
            let worker_script = self.config.cache_dir.join("veri_worker.py");
            cmd = Command::new(python_executable);
            cmd.arg(&worker_script);
        }

        cmd.env("VERI_WORKER_ID", worker.id.to_string())
            .arg("--worker-mode")
            .arg("--worker-id")
            .arg(worker.id.to_string())
            .arg("--cache-dir")
            .arg(&self.config.cache_dir)
            .arg("--work-dir")
            .arg(&self.config.work_dir)
            .current_dir(&self.config.work_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut process = cmd
            .spawn()
            .map_err(|e| anyhow!("Failed to start worker process {}: {}", worker.id, e))?;

        // Set up writer thread (stdin)
        let stdin = process
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Failed to open worker stdin"))?;
        let (tx, rx): (Sender<WorkerMessage>, Receiver<WorkerMessage>) = mpsc::channel();
        Self::spawn_writer_thread(stdin, rx);

        // Set up reader thread (stdout)
        if let Some(stdout) = process.stdout.take() {
            let evt_tx = self.evt_tx.clone();
            Self::spawn_reader_thread(stdout, worker.id, evt_tx);
        }
        // Drain stderr to logs
        if let Some(stderr) = process.stderr.take() {
            let evt_tx = self.evt_tx.clone();
            Self::spawn_stderr_logger(stderr, worker.id, evt_tx);
        }

        worker.sender = Some(tx);
        worker.process = Some(process);
        worker.state = WorkerState::Starting;
        worker.started_at = Instant::now();
        worker.last_activity = Instant::now();

        // Wait for HelloOk within startup timeout
        let deadline = Instant::now() + self.config.startup_timeout;
        loop {
            if Instant::now() > deadline {
                warn!("Worker {} did not send HelloOk in time", worker.id);
                // Mark failed so health checker can restart
                worker.state = WorkerState::Failed("Startup timeout".to_string());
                return Err(anyhow!("Worker {} startup timeout", worker.id));
            }
            match self.evt_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(WorkerEvent::HelloOk { worker_id }) if worker_id == worker.id => {
                    worker.state = WorkerState::Idle;
                    worker.last_activity = Instant::now();
                    break;
                }
                Ok(_) => {
                    // other event; ignore
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => {}
            }
        }

        debug!("Worker process {} started successfully", worker.id);
        Ok(())
    }

    fn find_py_worker_path(start: &Path) -> Option<PathBuf> {
        // Try start/py_worker and ascend a few levels
        let mut cur = start.to_path_buf();
        for _ in 0..5 {
            let cand = cur.join("py_worker");
            if cand.exists() {
                return Some(cand);
            }
            if !cur.pop() {
                break;
            }
        }

        // Try repo relative to this crate (../../py_worker)
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let cand = manifest_dir.parent().and_then(|p| p.parent()).map(|p| p.join("py_worker"));
        if let Some(c) = cand {
            if c.exists() {
                return Some(c);
            }
        }
        None
    }

    fn spawn_writer_thread(mut stdin: std::process::ChildStdin, rx: Receiver<WorkerMessage>) {
        thread::spawn(move || {
            while let Ok(msg) = rx.recv() {
                let json = match msg {
                    WorkerMessage::ExecuteTests { batch_id, nodeids, options } => {
                        let junit = options
                            .junit_xml
                            .as_ref()
                            .map(|p| p.to_string_lossy().to_string());
                        serde_json::json!({
                            "t": "ExecuteTests",
                            "batch_id": batch_id,
                            "nodeids": nodeids,
                            "options": {
                                "verbose": options.verbose,
                                "quiet": options.quiet,
                                "no_capture": options.no_capture,
                                "exitfirst": options.exitfirst,
                                "maxfail": options.maxfail,
                                "junit_xml": junit,
                                "workers": options.workers.clone().unwrap_or_else(|| "1".to_string()),
                                "coverage": options.coverage,
                                "coverage_xml": options.coverage_xml,
                                "coverage_html": options.coverage_html,
                                "coverage_source_dirs": options.coverage_source_dirs,
                                "coverage_omit": options.coverage_omit,
                            }
                        })
                    }
                    WorkerMessage::Shutdown => serde_json::json!({"t":"Shutdown"}),
                    WorkerMessage::HealthCheck => serde_json::json!({"t":"HealthCheck"}),
                };
                let line = serde_json::to_string(&json).unwrap_or_else(|_| "{}".to_string()) + "\n";
                let _ = stdin.write_all(line.as_bytes());
                let _ = stdin.flush();
            }
        });
    }

    fn spawn_reader_thread(
        stdout: std::process::ChildStdout,
        worker_id: usize,
        evt_tx: Sender<WorkerEvent>,
    ) {
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().flatten() {
                if let Ok(val) = serde_json::from_str::<Value>(&line) {
                    let t = val.get("t").and_then(|v| v.as_str()).unwrap_or("");
                    match t {
                        "HelloOk" => {
                            debug!("Worker {} HelloOk: {}", worker_id, line);
                            let _ = evt_tx.send(WorkerEvent::HelloOk { worker_id });
                        }
                        "HealthOk" => {
                            debug!("Worker {} HealthOk", worker_id);
                            let _ = evt_tx.send(WorkerEvent::HealthOk { worker_id });
                        }
                        "TestResults" => {
                            let batch_id = val.get("batch_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let exit_code = val.get("exit_code").and_then(|v| v.as_i64()).unwrap_or(3) as i32;
                            let stdout_s = val.get("stdout").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let stderr_s = val.get("stderr").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let duration_ms = val.get("duration_ms").and_then(|v| v.as_i64()).unwrap_or(0) as u64;
                            let per_test = val.get("per_test").and_then(|arr| arr.as_array()).map(|arr| {
                                arr.iter().filter_map(|x| {
                                    let nodeid = x.get("nodeid").and_then(|v| v.as_str())?.to_string();
                                    let outcome = x.get("outcome").and_then(|v| v.as_str())?.to_string();
                                    let duration_ms = x.get("duration_ms").and_then(|v| v.as_i64()).unwrap_or(0) as u64;
                                    Some(PerTestResult { nodeid, outcome, duration_ms })
                                }).collect::<Vec<PerTestResult>>()
                            });
                            let resp = WorkerResponse::TestResults { batch_id, exit_code, stdout: stdout_s, stderr: stderr_s, duration_ms, per_test };
                            let _ = evt_tx.send(WorkerEvent::TestResults { worker_id, response: resp });
                        }
                        "Error" => warn!("Worker {} error: {}", worker_id, line),
                        _ => debug!("Worker {} msg: {}", worker_id, line),
                    }
                } else {
                    debug!("Worker {} output: {}", worker_id, line);
                }
            }
        });
    }

    fn spawn_stderr_logger(
        stderr: std::process::ChildStderr,
        worker_id: usize,
        _evt_tx: Sender<WorkerEvent>,
    ) {
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().flatten() {
                warn!("Worker {} stderr: {}", worker_id, line);
            }
        });
    }

    /// Get the Python executable path
    fn get_python_executable(&self) -> Result<String> {
        // Try common Python executable names
        let candidates = ["python3", "python", "py"];

        for candidate in &candidates {
            if let Ok(output) = Command::new(candidate).arg("--version").output() {
                if output.status.success() {
                    return Ok(candidate.to_string());
                }
            }
        }

        Err(anyhow!("Could not find Python executable"))
    }

    /// Check worker health and restart failed workers
    fn check_worker_health(&mut self) -> Result<()> {
        // First pass: check worker health and mark failed workers
        let mut failed_workers = Vec::new();
        for (i, worker) in self.workers.iter_mut().enumerate() {
            // Check if process is still alive
            if let Some(process) = &mut worker.process {
                if let Ok(Some(exit_status)) = process.try_wait() {
                    warn!("Worker {} exited with status: {:?}", worker.id, exit_status);
                    worker.state =
                        WorkerState::Failed(format!("Process exited: {:?}", exit_status));
                    worker.process = None;
                }
            }

            // Heartbeat: if idle/busy and no activity for 10s, ping
            if matches!(worker.state, WorkerState::Idle | WorkerState::Busy(_))
                && worker.last_activity.elapsed() > self.config.heartbeat_interval
            {
                if let Some(sender) = &worker.sender {
                    let _ = sender.send(WorkerMessage::HealthCheck);
                }
            }

            // Collect failed workers for restart
            if worker.is_failed() && !self.shutdown_requested {
                failed_workers.push(i);
            }
        }

        // Second pass: restart failed workers
        for worker_idx in failed_workers {
            info!("Restarting failed worker {}", self.workers[worker_idx].id);

            // Get python executable before borrowing worker
            let python_executable = self.get_python_executable()?;
            let worker_script = self.config.cache_dir.join("veri_worker.py");
            let work_dir = self.config.work_dir.clone();
            let cache_dir = self.config.cache_dir.clone();

            // Restart worker in-place without calling separate method
            let worker = &mut self.workers[worker_idx];

            debug!("Starting worker process {}", worker.id);

            let mut cmd = Command::new(python_executable);
            cmd.arg(&worker_script)
                .arg("--worker-mode")
                .arg("--worker-id")
                .arg(worker.id.to_string())
                .arg("--cache-dir")
                .arg(&cache_dir)
                .current_dir(&work_dir)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            let process = cmd
                .spawn()
                .map_err(|e| anyhow!("Failed to start worker process {}: {}", worker.id, e))?;

            worker.process = Some(process);
            worker.state = WorkerState::Idle;
            worker.started_at = Instant::now();
            worker.last_activity = Instant::now();

            debug!("Worker process {} started successfully", worker.id);
        }

        Ok(())
    }

    /// Recycle idle workers to prevent memory leaks
    fn recycle_idle_workers(&mut self) -> Result<()> {
        if !self.config.enable_recycling {
            return Ok(());
        }

        let mut workers_to_recycle = Vec::new();
        for (i, worker) in self.workers.iter().enumerate() {
            if worker.should_recycle(self.config.max_idle_time) && !self.shutdown_requested {
                workers_to_recycle.push(i);
            }
        }

        for worker_idx in workers_to_recycle {
            debug!("Recycling idle worker {}", self.workers[worker_idx].id);

            // Get python executable before borrowing worker
            let python_executable = self.get_python_executable()?;
            let worker_script = self.config.cache_dir.join("veri_worker.py");
            let work_dir = self.config.work_dir.clone();
            let cache_dir = self.config.cache_dir.clone();

            let worker = &mut self.workers[worker_idx];

            // Shutdown old process
            if let Some(process) = &mut worker.process {
                let _ = process.kill();
                let _ = process.wait();
                worker.process = None;
            }

            // Start new process inline
            debug!("Starting worker process {}", worker.id);

            let mut cmd = Command::new(python_executable);
            cmd.arg(&worker_script)
                .arg("--worker-mode")
                .arg("--worker-id")
                .arg(worker.id.to_string())
                .arg("--cache-dir")
                .arg(&cache_dir)
                .current_dir(&work_dir)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            let process = cmd
                .spawn()
                .map_err(|e| anyhow!("Failed to start worker process {}: {}", worker.id, e))?;

            worker.process = Some(process);
            worker.state = WorkerState::Idle;
            worker.started_at = Instant::now();
            worker.last_activity = Instant::now();

            debug!("Worker process {} started successfully", worker.id);
        }

        Ok(())
    }

    /// Get pool statistics
    pub fn get_stats(&self) -> WorkerPoolStats {
        let idle_count = self
            .workers
            .iter()
            .filter(|w| matches!(w.state, WorkerState::Idle))
            .count();

        let busy_count = self
            .workers
            .iter()
            .filter(|w| matches!(w.state, WorkerState::Busy(_)))
            .count();

        let failed_count = self.workers.iter().filter(|w| w.is_failed()).count();

        WorkerPoolStats {
            total_workers: self.workers.len(),
            idle_workers: idle_count,
            busy_workers: busy_count,
            failed_workers: failed_count,
            queued_tasks: self.task_queue.len(),
            active_tasks: self.active_tasks.len(),
        }
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

/// Worker pool statistics
#[derive(Debug)]
pub struct WorkerPoolStats {
    pub total_workers: usize,
    pub idle_workers: usize,
    pub busy_workers: usize,
    pub failed_workers: usize,
    pub queued_tasks: usize,
    pub active_tasks: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_pool_creation() {
        let config = WorkerPoolConfig::default();
        let pool = WorkerPool::new(config);
        assert_eq!(pool.workers.len(), 0);
        assert_eq!(pool.task_queue.len(), 0);
        assert_eq!(pool.active_tasks.len(), 0);
    }

    #[test]
    fn test_worker_process_states() {
        let worker = WorkerProcess::new(0);
        assert!(matches!(worker.state, WorkerState::Starting));
        assert!(!worker.is_available());
        assert!(!worker.is_failed());
    }

    #[test]
    fn test_worker_pool_config_defaults() {
        let config = WorkerPoolConfig::default();
        assert!(config.worker_count > 0);
        assert!(config.startup_timeout > Duration::ZERO);
        assert!(config.execution_timeout > Duration::ZERO);
        assert!(config.enable_recycling);
    }
}
