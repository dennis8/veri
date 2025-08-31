# Troubleshooting Multi‑Worker

This guide lists common issues when running veri with multiple workers (`--workers N`) and how to diagnose and fix them.

- Tip: run with `-v` (or `-vv`) to print scheduling details and more diagnostics.
- Tip: set `RUST_LOG=info` (or `debug`) for additional internal logs.

## Quick Checklist
- Use the dev binary on PATH (in this repo: `.bin/veri`).
- Ensure pytest is importable in the environment the workers use (veri launches workers via `uv run --project py_worker`).
- If a plugin is blocked by allowlist, either add it to `allowed_plugins` in `veri.toml` or run with `--disable-allowlist`.
- Verify your test files follow pytest naming (e.g., `test_*.py`).

## Common Issues

### 1) “Worker X startup timeout”
- Meaning: veri started a Python worker but did not receive `HelloOk` within `startup_timeout_sec` (default 30s).
- Fix:
  - Check that `uv` is installed and on PATH: `uv --version`.
  - Confirm `py_worker/` exists in the project (veri detects it automatically).
  - Increase timeout in `veri.toml`:
    ```toml
    [worker]
    startup_timeout_sec = 60
    ```
  - Re-run with `RUST_LOG=debug` to see worker launch command and stderr.

### 2) “Worker not ready” or tasks never finish
- Meaning: the worker failed to initialize or crashed before accepting tasks.
- Fix:
  - Run with `RUST_LOG=debug` to inspect stderr.
  - Check Python/pytest availability inside the worker environment:
    - `uv run --project py_worker python -c "import pytest; print(pytest.__version__)"`.
  - Temporarily set `workers = 1` to confirm the test suite itself is healthy.

### 3) Allowlist blocks a plugin
- Symptom: usage error with message indicating blocked plugin.
- Fix options:
  - Add to `veri.toml`:
    ```toml
    [security]
    enforce_allowlist = true
    allowed_plugins = ["your-plugin"]
    ```
  - Or run with the CLI override (not recommended long‑term): `--disable-allowlist`.

### 4) “No tests found”
- Verify naming (e.g., `test_*.py`, `*_test.py`).
- Use `-a` (run all) to bypass change‑based selection during initial runs.
- Use `-k <expr>` to select by keyword; `-m <marker>` for markers.

### 5) Coverage: “No data was collected”
- Causes: tests exit early; coverage config excludes sources; running in a directory without Python sources.
- Fix:
  - Ensure `--cov` or `--cov-merge-full` is given and there are Python files under sources.
  - Verify `.coveragerc` and `source =`/`omit =` entries, or adjust `[worker]` coverage defaults.
  - For multi‑worker, veri combines `.coverage.worker_*` automatically; check `.veri/cache/coverage.json` exists.

### 6) Slow or unbalanced runs
- Run once with `-v` to write `.veri/cache/timings.json`.
- Subsequent runs load timings to balance batches.
- Ensure long tests have stable nodeids (parametrization can increase spread).
- Adjust batch cap in the scheduler if needed (currently set via internal defaults).

### 7) Heartbeats and timeouts
- If workers occasionally hang, tune:
  ```toml
  [worker]
  heartbeat_interval_sec = 5
  execution_timeout_sec = 600
  ```
- Use `RUST_LOG=debug` to see `HealthCheck` / `HealthOk` traffic.

## Useful Commands
- Print compatibility report and exit:
  ```bash
  veri --compatibility-report
  ```
- Verbose parallel run with 4 workers:
  ```bash
  RUST_LOG=info veri --workers 4 -v
  ```
- Full coverage (XML/JSON/HTML):
  ```bash
  veri --workers 4 --cov-merge-full
  ```

## Filing Issues
- Include: platform, Python/Rust versions, command invocation, `veri.toml`, and relevant output (`-v`, `RUST_LOG=debug`).
- For plugin compatibility problems, list your pytest plugins and versions.
