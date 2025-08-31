# Comprehensive Functional Test Plan for `veri`

## Goals
- Exercise real end-to-end behaviour across the Rust core, Python worker and CLI.
- Validate public JSON schemas and stable contracts.
- Cover concurrency, scheduling and file-system interactions without mocks.
- Maintain fast feedback by reusing temporary directories and small sample test projects.

## Current Coverage & Gaps
- CLI tests only verify flag parsing; they never spawn real commands or workers.
- Rust modules such as planner and import graph builder lack any tests.
- Worker pool, scheduler, cache and watch modules have only small unit tests and no integration runs.
- Python worker tests already exercise collection on temporary projects and can serve as a pattern for functional tests across the repo.

## Proposed Tests by Component

### 1. Python Worker (`crates/veri-core/src/python_worker.rs`)
- **Collect & Run Tests:** Run `collect_tests` and `run_tests` against a temporary project with multiple files, markers and fixtures to ensure subprocess invocation, exit codes and produced indexes.
- **Coverage & Options:** Execute runs with coverage flags and junit/xml options, verifying files are written to cache.
- **Error Handling:** Intentionally introduce failing tests, syntax errors, or missing worker script to confirm error paths return meaningful `anyhow!` messages.

### 2. Import Graph Builder (`crates/veri-core/src/import_graph.rs`)
- Build a tiny module graph with dynamic imports and validate that edges, dynamic imports and unresolved imports are captured.
- Cross-validate the generated graph using the Python `VeriASTParser` to guarantee parity.

### 3. Planner (`crates/veri-core/src/planner.rs`)
- Create a miniature project with known dependencies; modify source files and ensure `plan_test_selection` chooses the correct tests and broaden behaviour.
- Test rules for test files, `conftest.py` scopes and dynamic import safety.

### 4. Scheduler (`crates/veri-core/src/scheduler.rs`)
- **Deterministic Scheduling:** Provide historical timings and verify bin-packing assigns tests to workers based on priority, last-failed flags and estimated durations.
- **Strategy Variants:** Run `Fastest`, `FailFirst` and `Balanced` strategies and assert ordering of batches.
- **Property Testing:** Use `proptest` to generate random timing data and ensure total estimated duration per worker never exceeds `max_batch_duration_ms`.

### 5. Worker Pool (`crates/veri-core/src/worker_pool.rs`)
- Spin up real Python workers in parallel; submit batches and verify results, stdout/stderr and heartbeat events.
- Test recycling by idling workers longer than `max_idle_time` and ensuring processes restart.
- Simulate worker crash by killing a child process and ensure pool marks it failed and reschedules work.

### 6. Watch Mode (`crates/veri-core/src/watch.rs`)
- Create temporary directories with nested `__pycache__` and ignored patterns; verify that watch sessions skip ignored paths and trigger rebuild after debounce window.
- Test integration with `TestPlanner` and `PythonWorker` by modifying a source file and asserting only impacted tests run.

### 7. Cache (`crates/veri-core/src/cache.rs`)
- Build real cache keys by writing `uv.lock`, multiple `conftest.py` files and pytest plugins; ensure `compute_hash` changes when any component changes.
- Verify `find_conftest_files` skips ignored directories and handles large directory trees.

### 8. Event Stream & Telemetry (`crates/veri-core/src/event_stream.rs`, `telemetry.rs`)
- Produce a full test run, write JSONL events and validate against `schemas/event.jsonl.json` for every event type.
- Confirm `telemetry` counters increment on success/failure paths.

### 9. CLI Integration (`crates/veri-cli`)
- Use `assert_cmd` to run the compiled binary against the sample project:
  - `veri collect` – ensures indexes are produced.
  - `veri run` – executes tests, checks exit codes and output.
  - `veri watch` – start watcher, touch files and assert run triggers.
- Validate split/shard subcommands by comparing produced shard manifests.

### 10. Schema Validation (`schemas/`)
- Use Rust `jsonschema` crate or Python `jsonschema` to validate `tests.index.json`, `markers.index.json`, `imports.graph.json`, etc., ensuring cross-language compatibility.
- Add regression tests that load past schema versions to maintain backward compatibility.

### 11. Cross-Language & End-to-End
- Combine components: run CLI to collect, plan, schedule and execute tests while streaming events; then load outputs and assert that scheduler and planner decisions match expectations.
- Use the `examples/` directory as canonical end-to-end scenarios.

### 12. Performance & Stress
- Measure scheduler and worker pool throughput with `criterion` or timed integration tests.
- Run high-parallelism tests to detect race conditions or file-system watcher saturation.

### 13. Cross-Platform & Tooling
- Extend CI matrix to run on Linux, macOS and Windows to catch watcher or path issues.
- Use `cargo nextest` for Rust tests and `uv run pytest -v` for Python tests to ensure consistent environments.

## Continuous Integration Workflow
1. `cargo test --workspace` – Rust unit and integration tests.
2. `uv run pytest -v` inside `py_worker/` – Python tests.
3. `just check` – combined `fmt`, `lint`, and type checks.
4. Add a full end-to-end job running the CLI against a sample project to verify schemas and event streams.

By emphasising real subprocesses, file systems and JSON schemas, this plan drives the project toward robust functional coverage with minimal reliance on mocks.
