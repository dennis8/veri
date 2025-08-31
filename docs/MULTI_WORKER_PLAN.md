# Veri Multi‑Worker Execution Plan

This document details the end‑to‑end plan to make veri’s multi‑worker execution reliable, fast, and well‑tested. It covers architecture, protocols, implementation tasks across Rust and Python, diagnostics, test strategy, and milestones.

## Goals
- Parallelize test execution across N Python worker processes for speedups on multi‑core systems.
- Keep collection single‑shot (no per‑worker re‑collection); schedule nodeids to workers efficiently.
- Robust worker lifecycle: startup, health checks, backpressure, graceful shutdown, and auto‑restart.
- Deterministic, resumable, and cache‑friendly behavior.
- First‑class coverage support (per‑worker data + merge).
- Clear diagnostics and telemetry for visibility and debugging.

## Non‑Goals (for initial release)
- Cross‑machine distributed execution (future RFC).
- Preemptive task rebalancing mid‑test (we will support post‑batch rebalancing).
- Full plugin emulation for plugins that fundamentally hijack collection/execution (use `--engine pytest`).

---

## Current State Summary (as of 2025‑08‑31)
- ✅ Scheduler (`veri-core::scheduler`) computes batches correctly (unit tested).
- ✅ CLI has a parallel branch (`execute_tests_parallel`) and summary printing.
- ⚠️ WorkerPool scaffolding exists but lacks IPC (no stdin writer/stdout reader threads); sender is `None`, so assigning tasks can error with “Worker N is not ready”.
- ❌ Python shim (`py_worker/veri_worker.py`) has no `--worker-mode` event loop or JSONL protocol.
- ⚠️ CLI is currently guarded to single‑worker until WorkerPool is functional.

---

## Architecture

### Process Model
- Rust process (veri CLI)
  - Plans batches with `TestScheduler`.
  - Manages a `WorkerPool` of M Python worker subprocesses.
  - Sends “execute tests” commands with nodeids to a chosen worker.
  - Collects results, updates telemetry, merges coverage (optional), reports summary.

- Python worker process (py_worker)
  - Long‑lived “worker mode”: reads commands on stdin (JSON Lines), writes responses on stdout (JSON Lines), logs to stderr.
  - Executes pytest with the provided nodeids in the specified working directory.
  - Optionally handles coverage per worker.

### IPC: JSONL over stdio
- Transport: one JSON message per line on stdout/stderr; stdin for commands.
- Reasons: portable, fast enough, works on Windows/macOS/Linux, minimal deps.
- Add a versioned protocol and message schemas (appendix below). Keep the protocol in `schemas/worker_protocol@1.json`.

### Protocol Overview (v1)
- Commands (Rust → Python):
  - `Hello { worker_id, protocol: "1" }`
  - `ExecuteTests { batch_id, nodeids, options }`
  - `HealthCheck {}`
  - `Shutdown {}`

- Responses (Python → Rust):
  - `HelloOk { worker_id, py_version, pytest_version }`
  - `HealthOk { ts }`
  - `TestResults { batch_id, exit_code, stdout, stderr, duration_ms, nodeids, per_test?: [ { nodeid, outcome, duration_ms } ] }`
  - `Error { message, kind, details? }`
  - `Log { level, message }`  // optional informational logs routed via stdout channel

Notes:
- All messages have an envelope: `{ t: "<Type>", ... }` to match existing schema patterns.
- Keep messages < 1MB; chunk large stdout/stderr (or truncate with indicator) to prevent pipe blocking.

---

## Rust Implementation Plan

### WorkerPool Enhancements
- State model per worker:
  - `Starting` → `Idle` → `Busy(batch_id)`; failure puts it into `Failed(reason)`; on shutdown → `Shutdown`.
- Startup:
  - Spawn `python` with the cached `veri_worker.py` and `--worker-mode`.
  - Wrap stdio; spawn reader thread to parse JSONL responses and forward to a `crossbeam_channel::Sender<WorkerEvent>`.
  - Send `Hello { worker_id }`; await `HelloOk` within `startup_timeout`.
- Command routing:
  - Maintain `Sender<Command>` per worker; `assign_task_to_worker()` sends `ExecuteTests`.
  - Backpressure: `task_queue` remains; do not overfill worker command pipes if they’re busy.
- Health checks:
  - Timer to send `HealthCheck` every `N` seconds; if no `HealthOk` within `heartbeat_timeout`, mark worker failed and restart.
- Results & completion:
  - Reader thread emits `WorkerEvent::Result(BatchResult)`; pool aggregates results, updates stats.
  - `wait_for_completion(timeout)` loops until all batches done or timeout; returns vector of results.
- Auto‑restart:
  - If worker exits/crashes, mark `Failed`, restart up to `k` times with exponential backoff and requeue the active batch.

### Types and Threads
- Add:
  - `enum WorkerCommand { ExecuteTests{..}, HealthCheck, Shutdown }`
  - `enum WorkerEvent { HelloOk{..}, HealthOk{..}, TestResults{..}, Error{..}, Log{..}, Exited{status} }`
  - Channels: `(cmd_tx, cmd_rx)` per worker; `(evt_tx, evt_rx)` for pool‑wide events.
- Reader thread per worker:
  - Read stdout lines; `serde_json::from_str` → `WorkerEvent` → `evt_tx`.
  - Map stderr lines to `WorkerEvent::Log{level: "ERROR"}` for visibility.

### Scheduling Integration
- Reuse existing `TestScheduler` batches.
- Submit batches to `WorkerPool` immediately (`process_queue()`), track in `active_tasks`.
- Optional: small batch size (e.g., cap estimated 30s per batch) to smooth load balance.
- Optional Phase 2+: “long pole splitting” – if a worker batch is predicted long, split before dispatch.

### Coverage Integration
- Test run options include `coverage` flags already. For multi‑worker:
  - Each worker writes coverage to `cache/.coverage.worker_<id>` and `cache/coverage.worker_<id>.json`.
  - After all results, CLI merges: `coverage combine` (via py shim) or merges JSON with a Rust collector.
  - Preserve incremental behavior: keep per‑test timing to improve future scheduling.

### Diagnostics & Telemetry
- Add diagnostics for:
  - Worker startup timeout.
  - IPC decode error.
  - Batch execution timeout.
  - Auto‑restart exceeded.
- Emit event stream (existing JSONL) per batch start/finish; record failures with categories.

### CLI and Config
- Keep `--workers [auto|N]`. When N>1, enable `WorkerPool`; else single worker path stays.
- Add advanced (config or env) knobs:
  - `worker.startup_timeout`, `worker.heartbeat_interval`, `worker.execution_timeout`, `worker.max_restarts`.
- Execution now always uses the worker pool (workers can be set to 1 for single‑process execution). The old experimental gate has been removed.

---

## Python Implementation Plan

### Worker Mode Event Loop
- Extend `veri_worker.py` to support `--worker-mode`:
  - On start, print `HelloOk { worker_id, py_version, pytest_version }`.
  - Enter loop: read JSON lines from stdin; handle commands.
  - On `ExecuteTests`:
    - Build pytest args (respect `verbose`, `-s`, `-x`, `--maxfail`, `-n 1` within the worker process).
    - If coverage enabled, use a per‑worker data file under `.veri/cache`.
    - Time the run; capture exit code; optionally capture per‑test durations via pytest hooks.
    - Emit `TestResults` with summary + optional per‑test details; flush stdout.
  - On `HealthCheck`: emit `HealthOk` with timestamp.
  - On `Shutdown`: finalize coverage (if running), flush, and exit 0.

### Robustness
- Make reads/writes line‑buffered; ensure UTF‑8; guard against partial writes.
- Enforce max payload sizes (truncate stdout/stderr in `TestResults` with a note when exceeded).
- Handle KeyboardInterrupt and exceptions: emit `Error { kind: "UnhandledException" }` then exit non‑zero.

### Coverage Changes
- Accept `--cache-dir` and `--worker-id` to derive unique coverage data file.
- For JSON output, write `coverage.worker_<id>.json`.

### Cross‑Platform
- Avoid `select` on pipes; just blocking I/O is fine (Rust reader thread unblocks).
- Ensure path handling works on Windows; avoid relying on `fork` semantics.

---

## Error Handling & Recovery
- Startup: if no `HelloOk` within timeout → restart; if exceeds `max_restarts` → abort with diagnostic.
- Execute: if worker exits mid‑batch → mark batch failed, requeue once; after retry fail → propagate error.
- Health: missed heartbeats → soft mark, then restart.
- IPC: JSON decode error → request worker self‑report; if persistent → restart.

---

## Testing Strategy

### Unit Tests (Rust)
- `worker_pool.rs`:
  - State transitions (Starting → Idle → Busy → Idle).
  - Backpressure and queue processing.
  - Restart logic capped by `max_restarts`.
- `scheduler.rs`:
  - Small batches, bin packing, long‑pole detection logic.

### Unit/Functional (Python)
- Worker mode loop with a fake command stream; verify responses for ExecuteTests, HealthCheck, Shutdown.
- Coverage start/stop producing per‑worker files.

### Integration Tests
- Spin up 2–4 workers against example suites; assert:
  - All tests run; results match single‑worker exit behavior.
  - Coverage artifacts per worker produced and merged.
  - HealthCheck path works by injecting a hung worker (env flag to simulate delay) and verifying restart.

### E2E
- `veri -v --workers auto examples/` on CI matrix (linux, macOS, windows).
- Flaky test retry path still works with multiple workers (retry inside the assigned worker).

---

## Rollout Plan & Milestones

### Phase 1: Protocol + Basic Pool (scaffolding)
- Implement JSONL protocol schema and Python worker `--worker-mode` loop supporting: startup `HelloOk`, `ExecuteTests`, `HealthCheck`, `Shutdown`.
- Add Rust WorkerPool IPC scaffolding: writer thread (serialize `WorkerMessage` → JSONL) and reader thread (parse responses → log/queue).
- Do not enable multi‑worker yet in CLI (keeps guard). Validate protocol manually via ad‑hoc run.

### Phase 2: Wire‑up + Reliability + Coverage
- Connect `assign_task_to_worker` to writer thread; enqueue commands for real batches.
- Update `poll_results/wait_for_completion` to drain reader events and materialize `BatchResult`.
- Heartbeats, startup timeout, execution timeout, restarts.
- Per‑worker coverage files + post‑run merge in CLI.
- Diagnostics for common failures; telemetry events.

### Phase 3: Performance + Balancing
- Batch sizing enforced with bin‑packing cap; long‑pole threshold respected in scheduling stats.
- Historical timings used to balance batches and avoid long poles.
- Example suites measured; per‑test timings persisted to improve next runs.

### Phase 4: CI Readiness (Completed)
- CI matrix (Linux/macOS/Windows) added; smoke tests for multi‑worker.
- Experimental gate removed; `--workers N` is production path. Docs updated (QUICKSTART, SPEC).

### Definition of Done
- `just check` green: fmt, lint, build, tests (Rust + Python).
- Examples run clean with `--workers 2`.
- Coverage merged across workers when `--cov`.
- Docs updated (SPEC.md, QUICKSTART.md, TROUBLESHOOTING) with multi‑worker notes.

---

## Acceptance Criteria
- Running `veri --workers 4` on examples yields equal or faster wall time vs `--workers 1`.
- No “Worker N is not ready” or deadlocks under normal operation.
- Recover from one worker crash without losing the entire run.
- Diagnostics clearly point to remediation when environment is misconfigured.

---

## Appendix: Message Examples (Protocol v1)

Command (Rust → Python):
```json
{ "t": "Hello", "worker_id": 0, "protocol": "1" }
```

Response:
```json
{ "t": "HelloOk", "worker_id": 0, "py_version": "3.12.4", "pytest_version": "8.3.2" }
```

Execute:
```json
{
  "t": "ExecuteTests",
  "batch_id": "batch_3",
  "nodeids": ["tests/test_calc.py::test_add[1-2]", "tests/test_calc.py::test_sub"],
  "options": {
    "verbose": true,
    "no_capture": false,
    "exitfirst": false,
    "maxfail": null,
    "junit_xml": null,
    "workers": "1",
    "coverage": true,
    "coverage_xml": false,
    "coverage_html": false,
    "coverage_source_dirs": ["src"],
    "coverage_omit": ["*/tests/*", "*/__pycache__/*"]
  }
}
```

TestResults:
```json
{
  "t": "TestResults",
  "batch_id": "batch_3",
  "exit_code": 0,
  "stdout": "..\n2 passed\n",
  "stderr": "",
  "duration_ms": 248,
  "nodeids": ["tests/test_calc.py::test_add[1-2]", "tests/test_calc.py::test_sub"],
  "per_test": [
    { "nodeid": "tests/test_calc.py::test_add[1-2]", "outcome": "passed", "duration_ms": 110 },
    { "nodeid": "tests/test_calc.py::test_sub", "outcome": "passed", "duration_ms": 87 }
  ]
}
```

Error:
```json
{ "t": "Error", "kind": "UnhandledException", "message": "Traceback ..." }
```

---

## Work Breakdown (PR‑sized Tasks)
1. Protocol schema + Python worker mode loop (Hello/Health/Shutdown) + tests.
2. Rust WorkerPool IPC: reader thread, event channel, startup handshake.
3. ExecuteTests path end‑to‑end with one batch; verify CLI receives results.
4. Batch queueing + multiple workers + wait_for_completion; basic scheduling.
5. Timeouts, restarts, and diagnostics with unit tests.
6. Per‑worker coverage file naming + merge step in CLI.
7. Windows/macOS CI runs; fix platform issues.
8. Documentation updates (SPEC/QUICKSTART/TROUBLESHOOTING) and examples.
