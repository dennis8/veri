//! Worker process pool for parallel test execution
//!
//! This module provides a managed pool of Python worker processes that can
//! execute tests in parallel while maintaining warm interpreter state for
//! improved performance.

use anyhow::{anyhow, Context, Result};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Sender;
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
    },
    HealthOk,
    Error {
        message: String,
    },
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
    sender: Option<Sender<WorkerMessage>>,
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
}

impl WorkerPool {
    /// Create a new worker pool
    pub fn new(config: WorkerPoolConfig) -> Self {
        Self {
            config,
            workers: Vec::new(),
            task_queue: VecDeque::new(),
            active_tasks: HashMap::new(),
            shutdown_requested: false,
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
        let results = Vec::new();

        // Check for completed tasks and worker health
        self.check_worker_health()?;
        self.recycle_idle_workers()?;

        // Process any newly completed results
        // Note: In a real implementation, this would involve checking
        // message channels or shared state from worker processes

        // Process queue again in case workers became available
        self.process_queue()?;

        Ok(results)
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

        // Build command to start Python worker
        let python_executable = self.get_python_executable()?;
        let worker_script = self.config.cache_dir.join("veri_worker.py");

        let mut cmd = Command::new(python_executable);
        cmd.arg(&worker_script)
            .arg("--worker-mode")
            .arg("--worker-id")
            .arg(worker.id.to_string())
            .arg("--cache-dir")
            .arg(&self.config.cache_dir)
            .current_dir(&self.config.work_dir)
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

        // TODO: Set up communication channels with the worker process
        // For now, we'll use a placeholder

        debug!("Worker process {} started successfully", worker.id);
        Ok(())
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
