# Implementation Gap Remediation Plan

This document outlines a step-by-step plan to address the outstanding issues identified during the code review of the current repository. Each section lists the problem, the desired outcome, and concrete actions required to close the gap.

## 1. Telemetry placeholders

**Problem:** Telemetry collection uses placeholders for the collection timestamp, Python version, and last-failed test tracking. Metrics are gathered but never transmitted.

**Goal:** Produce accurate telemetry records and optionally send them when enabled.

**Relevant code:** `crates/veri-core/src/telemetry.rs`, `crates/veri-cli/src/main.rs`

**Actions:**
1. Capture the actual collection start time with `SystemTime` and record it in telemetry events.
2. Detect the Python version executed by each worker and include it in the telemetry payload.
3. Persist and reuse `last-failed` information so that `--last-failed` runs work between sessions.
4. Implement a small async client that POSTs telemetry batches to the configured endpoint with exponential backoff and `--no-network` support.
5. Add unit tests for telemetry serialization and an integration test that exercises sending to a local HTTP server.

## 2. Cache key generation

**Problem:** Cache keys rely on hard-coded placeholder values (Python version, site-packages digest, pytest version, plugin list), risking unnecessary cache invalidation or collisions.

**Goal:** Generate deterministic cache keys that reflect the true execution environment.

**Relevant code:** `crates/veri-core/src/cache.rs`

**Actions:**
1. Expose a `PythonEnvironment` helper that surfaces interpreter path, version, and resolved site-packages directory.
2. Hash the contents of `uv.lock` and the resolved site-packages directory to derive `site_packages_digest`.
3. Query pytest for its version and enumerated plugins via `pytest --version --plugins` and fold the result into the key.
4. Ensure `veri --explain` prints each component of the key for troubleshooting.
5. Add unit tests that mutate each component and assert key changes.

## 3. Watch mode TUI and globbing

**Problem:** Watch mode currently has a stub TUI and relies on a home-grown glob matcher that only supports `*` wildcards.

**Goal:** Provide a minimal but useful TUI and robust path filtering.

**Relevant code:** `crates/veri-core/src/watch.rs`, `crates/veri-cli/src/main.rs`

**Actions:**
1. Adopt `ratatui` for a cross-platform TUI: display progress, failing tests, and hints about impacted files.
2. Replace the custom glob matcher with `globset` to gain support for `?`, `**`, character classes, and proper escaping.
3. Document keyboard shortcuts for cancelling runs or toggling verbose output.
4. Add integration tests for glob filtering and snapshot tests for basic TUI rendering.

## 4. Interpreter configuration

**Problem:** `PythonWorker` invokes the `python` binary directly, which can mismatch the interpreter chosen by `uv` or user configuration.

**Goal:** Always spawn workers with the interpreter resolved by the CLI or configuration.

**Relevant code:** `crates/veri-core/src/python_worker.rs`

**Actions:**
1. Pass the resolved interpreter path from the CLI to the worker pool.
2. Use that path for environment checks and execution instead of hard-coded `python`.
3. Add a regression test that runs with an alternate interpreter path to ensure it is respected.

## 5. AST parser maintenance

**Problem:** The parser uses a manually curated list of standard-library modules to decide which imports are "safe," which will drift over time.

**Goal:** Maintain an up-to-date and automated list of stdlib modules.

**Relevant code:** `py_worker/veri_worker.py`

**Actions:**
1. At build time, generate the stdlib module list by invoking `python - <<'PY'
import json, sys
print(json.dumps(sorted(sys.stdlib_module_names)))
PY` and embed it in the binary.
2. Fall back to a runtime query when the list is missing or when `--explain-stdlib` is set for debugging.
3. Add tests that ensure the embedded list loads and that imports of known stdlib modules are treated as non-project dependencies.

## 6. Telemetry transmission stub

**Problem:** Even when telemetry is enabled, the code logs "Would send telemetry..." instead of transmitting data.

**Goal:** Deliver telemetry to the endpoint in a non-blocking manner while respecting opt-out flags.

**Relevant code:** `crates/veri-core/src/telemetry.rs`

**Actions:**
1. Reuse the async client from section 1 to batch and send telemetry.
2. Queue telemetry events and flush them at the end of the run or when the queue reaches a size threshold.
3. Ensure errors are logged at a low verbosity level and do not fail the test run.
4. Provide an environment variable (`VERI_TELEMETRY_DEBUG`) that logs raw payloads for troubleshooting.

## Verification

* Run `cargo test --workspace` and `uv run pytest -v` to ensure all new code paths are covered.
* Add documentation for each new flag or behavior in `README.md` and `TELEMETRY.md` as implementations land.

---

Tracking these tasks in GitHub issues or a project board is recommended so progress on each item is visible and testable.
