Love it. Here’s a concrete, end-to-end plan to turn **veri** into an Astral-grade, ultra-fast Python test runner (pytest-compatible) that feels instant, predictable, and CI-ready.

# 1) Product thesis

* **Promise:** “Run only what matters, immediately.”
* **Scope:** A drop-in test runner for Python projects that accelerates collection, selection, and execution—without giving up pytest compatibility.
* **Constraints:** Single static binary (Rust core), zero-config by default, deterministic caching, first-class Windows/PowerShell + Linux + macOS.

# 2) Primary users & “aha” moments

* **App & library devs:** instant feedback loop; `veri -w` watch mode under a second for typical edits.
* **Data/analytics (dbt/Dagster):** large test trees; selective runs by impacted DAG nodes or import graph.
* **CI owners:** stable shards, flaky isolation, JUnit/coverage artifacts, big wall-clock reductions.

# 3) Core UX (the 90% path)

```bash
# drop-in
veri               # run changed-or-impacted tests, fail-first
veri -a            # run all tests (like pytest)
veri -k "expr"     # filter by nodeid substring/markers
veri -w            # watch mode; reruns only impacted tests
veri --last-failed # rerun last failures first
veri --cov         # incremental + full merged coverage
veri split --ci N  # produce N balanced shards (by historical timings)
veri shard --ci i  # run shard i with stable assignment
```

Config is optional. If needed:

```toml
# veri.toml
[run]
workers = "auto"        # or 8
fail_first = true
timeout_sec = 600

[selection]
mode = "impacted"       # changed|impacted|all
respect_markers = true
exclude = ["tests/slow/**"]

[ci]
junit_xml = "reports/junit.xml"
coverage_xml = "reports/coverage.xml"

[caching]
dir = ".veri/cache"
key_env = ["PYTHONPATH", "UV_LOCKHASH"]  # env bits folded into cache key
```

# 4) Architecture (high level)

* **Rust CLI (core)**

  * Fast filesystem scan (glob, gitignore aware), hashing (BLAKE3), debounce watch.
  * Static analysis: Python **AST import graph** + conftest/fixture touchpoints.
  * Selection planner: decide the *minimal* safe subset to run; fall back to all if uncertain.
  * Scheduler: multi-process pool (spawn Python workers), “fail-first”, dynamic load balancing by historical timings.
  * Reporters: dot/spec/progress/TUI; emit JUnit XML & JSONL events; coverage integration.
* **Python worker (thin)**

  * Executes tests through pytest **compat shim**:

    * First run: full pytest collection → persist nodeids, markers, file↔nodeid map, plugin list.
    * Subsequent runs: **incremental collection** (only changed/impacted files) or reuse cached nodeids.
  * Coverage hook (coverage.py) with **incremental map**; merges to full report on demand.
* **Caches (content-addressed)**

  * Keys include: Python version, OS, `pip/uv` lock digest, pytest + plugin versions, `conftest.py` and helpers’ digests.
  * Artifacts: nodeid index, import graph, file→tests map, historical timings, coverage map.

# 5) Test discovery & selection (the speed engine)

**Goal:** 0 collection on hot path; sub-200ms scheduling for small edits.

* **Initial run (cold):**

  * Use pytest to collect full nodeids/markers/fixtures once.
  * Build **import graph** for `src/**` and `tests/**` via AST (no imports executed).
  * Persist:

    * `tests.index` → `{file → [nodeids]}`
    * `imports.graph` → directed edges module→imports (with file→module map)
    * `fixtures.map` → discovered fixture providers (from conftests/plugins)
* **Hot path (edit):**

  * If **test file** changed → run its nodeids immediately.
  * If **src file** changed → find reverse-deps via import graph to relevant tests.
  * If risky change (e.g., `conftest.py`, plugin list, site-customize) → escalate to partial/full invalidation.
* **Safety:** If anything looks ambiguous (e.g., dynamic import, `exec`, monkeypatch of import paths), **fall back to broader selection** and note why (great DX).

# 6) Parallelism & scheduling

* **Workers:** N = cores by default; `--workers N` override. Sticky processes re-used to avoid interpreter warmup.
* **Batching:** Group short tests to amortize startup; cap long tests per worker.
* **Fail-first:** Move red tests to front; immediate feedback in watch mode.
* **Historical timings:** Persist per nodeid mean/p95; use to **balance shards** and schedule long poles earlier.
* **Resource tags:** Optional marker (e.g., `@pytest.mark.db`) maps to **concurrency lanes** (e.g., only 1 DB test at a time).

# 7) Coverage (fast & incremental)

* **Incremental measurement:** Only for selected tests; maintain a **coverage union** artifact for the branch.
* **Reports:** HTML (local), XML (CI). Thresholds can be **global and per-diff** (changed lines gate).
* **No surprise slowness:** Coverage off by default in watch; one flag to merge full for CI (`veri --cov --merge-full`).

# 8) Watch mode (developer delight)

* **File watcher:** cross-platform, Git-aware (ignores `.git/`, `.venv/` etc.), debounced.
* **Behavior:** Interrupt running batch if a file saves; replay new plan quickly.
* **TUI:** minimal: failures list, progress bar, hints (“2 tests impacted by src/parser.py”).

# 9) Compatibility stance

* **Python:** 3.9–3.13+ (arm64 & x86\_64).
* **OS:** Windows/PowerShell, Linux, macOS.
* **Pytest compatibility:**

  * **MVP:** core pytest features: markers, xfail, parametrization, fixtures, `-k` expressions.
  * **Known tricky plugins (MVP defer):** plugins that mutate collection deeply, or require custom schedulers. Provide **escape hatch**: `veri --engine pytest` → hand off fully to pytest for a single run (still use reporters & timings).

# 10) CI integration (drop-in)

* **Artifacts:** JUnit XML, coverage XML/HTML, JSONL event stream.
* **Sharding:** `veri split --ci $N > manifest.json` (balanced by timings); each runner `veri shard --ci $i --manifest manifest.json`.
* **Example (GitHub Actions, Linux & Windows):**

```yaml
- name: Cache veri
  uses: actions/cache@v4
  with:
    path: .veri/cache
    key: ${{ runner.os }}-${{ hashFiles('uv.lock', 'pyproject.toml', '.veri/cachekey') }}

- name: Run tests (shard)
  run: veri --ci --cov shard --ci ${{ matrix.shard }}
```

# 11) Performance targets (engineering OKRs)

* **P0:** Hot edit → result in **<300ms** for small suites; **<2s** for medium suites (selection + first batch output).
* **P1:** Cold start vs pytest: **≥2× faster collection**; **≥20%** wall-clock win on “run all”.
* **P2:** CI: **≥30%** median reduction via selective re-runs on PRs + balanced sharding.

*(Targets are stretch but guide design; always fail-safe to correctness.)*

# 12) Telemetry & trust

* Off by default. When enabled: counts/timings only, no PII/nodeids.
* `--no-network` hard-block.
* Reproducibility: every run logs **cache keys** and invalidation reasons.

# 13) Risks & mitigations

* **Risk:** Pytest plugin edge cases.
  **Mitigation:** strict compatibility matrix; detect misfit and auto-fallback with a friendly message + link.
* **Risk:** Incorrect impact analysis → missed tests.
  **Mitigation:** conservative graph + heuristics; broaden selection when unsure; nightly “full sweep” CI job recommended.
* **Risk:** Windows path/TTY quirks.
  **Mitigation:** PowerShell-first CI; broad path tests; no reliance on POSIX-only features.

# 14) Open source & commercial shape

* **OSS (MIT/Apache-2):** core runner, selection engine, reporters, coverage integration, watch, split/shard, JSONL events.
* **Team/Enterprise (add-ons):**

  * **Remote cache** (share timings/node maps across the org),
  * **Flaky quarantine** (auto-detect flakiness, quarantine list + reports),
  * **Central dashboard** (timings/flake rate per nodeid, PR annotations).

# 15) Minimal data formats (stable and friendly)

* `.veri/tests.index` (JSONL) — `{"file":"tests/test_x.py","nodeids":["...::test_a", "...::test_b"]}`
* `.veri/imports.graph` (binary or JSON) — module graph (file→imports\[])
* `.veri/timings.json` — `{nodeid: {mean_ms, p95_ms, last_ms, count}}`
* `.veri/coverage.map` — per-file bitset/line ranges (mergeable)
* Keys hashed with **BLAKE3**; include: interpreter, OS, `uv.lock` digest, pytest+plugin versions, conftest digests.

# 16) Implementation roadmap

**Phase 0 – Spike (1–2 weeks)**

* Rust CLI skeleton (argparse, logging, colored output).
* Python worker that shells out to pytest for collection; persist `tests.index`.
* Run tests by nodeid list; basic parallel pool; dot reporter.
* Watch mode with debounce; changed test files only.

**Phase 1 – MVP (4–6 weeks)**

* AST import graph + safe reverse-dep selection.
* Historical timings + fail-first scheduling.
* Basic coverage integration (incremental + merge).
* JUnit XML, JSONL events; GH Actions annotations.
* Windows/macOS/Linux parity; quickstart docs; `pipx/uvx` install.

**Phase 2 – Beta**

* Smarter invalidation (fixtures/conftest awareness).
* Stable CI sharding (balanced by timings); manifest format.
* TUI (failures list, progress).
* Plugin compatibility list + auto-fallback.

**Phase 3 – v1**

* Flaky detector (rerun on failure N=1; track variance).
* Resource lanes via markers (e.g., `@db`, `@net`).
* Diff-coverage gate (changed lines).
* Remote cache (opt-in), team features.

# 17) Testing strategy

* **Self-host:** dogfood on veri itself (meta-tests).
* **Canary repos:** fastapi, pydantic, polars, sqlalchemy (public OSS with CI).
* **Matrix:** Py 3.9–3.13, Windows/Linux/macOS; with/without venv; with uv/pip.

# 18) Developer ergonomics

* **Install:** `uv tool install veri` (or static binary download).
* **No config needed**; first run prints what it decided and why:
  “Changed: `src/foo.py` → impacted: `tests/test_bar.py::test_baz` (via import graph).”
* **Escape hatches:** `--engine pytest` (full handoff), `--mode all`, `--no-cache`.

# 19) Example 5-minute demo script

1. Clone a medium Python project with \~1k tests.
2. `uv tool install veri`
3. `veri` → cold: collects once, runs all (note wall-clock).
4. Edit `src/parser.py` → `veri -w` reacts; shows **2 impacted tests**, finishes in sub-second.
5. `veri --last-failed` shows red tests first; fix; auto-rerun.
6. `veri split --ci 4` + `veri shard --ci 1` demo balanced shards; show JUnit + coverage artifacts.

# 20) “Astral-grade polish” checklist

* Great error copy (“Why didn’t my test run?” → exact reason).
* `--explain` dumps decision tree + cache keys.
* Deterministic ordering (stable seeds, sorted nodeids).
* PowerShell-friendly output (no ANSI glitches by default).
* Works fully offline after first run (cache warm).

---

## Next steps (actionable)

* Pick **MVP boundaries**: *pytest core only*, **no plugin shims** beyond basics; import graph + nodeid cache; timings; watch; JUnit.
* I’ll draft:

  1. CLI spec (`veri help` tree + flags),
  2. Cache schema v0,
  3. Minimal Rust crate layout + Python worker stub,
  4. A GitHub Actions example for your repos.

If you want, I can also sketch the **AST import graph** algorithm and invalidation rules as a short, commented pseudocode block to hand to an engineer.
