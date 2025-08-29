Awesome—here’s a concrete **veri competitive spec** you can hand to an engineer or shape into an RFC. It maps pytest → veri, nails the import-impact algorithm, and shows CI replacements (GH Actions, GitLab, Azure). I kept the design “Astral-grade”: single binary, fast path first, deterministic caching, Windows-friendly.

---

# 1) Side-by-side flags (pytest ⇄ veri)

| Theme                        | Pytest (today)                    | veri (drop-in)                             | Notes                                                             |
| ---------------------------- | --------------------------------- | ------------------------------------------- | ----------------------------------------------------------------- |
| Run all                      | `pytest`                          | `veri -a`                                  | `-a/--all` = full collection+run (uses veri’ fast collector)     |
| Changed/impacted             | (n/a) \| `pytest-testmon`         | `veri`                                     | Default: impacted set from static graph, fallback safe to broader |
| Watch                        | plugins: `pytest-watcher`, `ptw`  | `veri -w`                                  | Impact-aware, sub-second incremental plan                         |
| Select expression            | `-k "expr"`                       | `-k "expr"`                                 | Same semantics, applied after veri selection                     |
| Markers                      | `-m "slow and not net"`           | `-m "slow and not net"`                     | Same semantics; cached marker index                               |
| Fail fast                    | `-x` or `--maxfail=N`             | `-x` or `--maxfail=N`                       | Same                                                              |
| Verbosity                    | `-q`, `-vv`                       | `-q`, `-vv`                                 | Same; quiet keeps UX terse for CI                                 |
| Capture                      | `-s` / `--capture=no`             | `--no-capture`                              | Same effect                                                       |
| Exit on first error in watch | `--maxfail=1` + plugin            | `-w` (default fail-first)                   | Watch mode brings red upfront automatically                       |
| Parallel (local)             | `-n auto` (xdist)                 | `--workers auto`                            | veri: one collection → many workers (no per-worker re-collect)   |
| Sharding (CI multi-machine)  | plugin `pytest-split` + durations | `veri split --ci N` + `veri shard --ci i` | Stable, timing-aware shards from one manifest                     |
| Last failed                  | `--last-failed`                   | `--last-failed`                             | Same semantics; uses veri cache                                  |
| JUnit report                 | `--junitxml=path`                 | `--junit-xml=path`                          | Same output schema (team tools keep working)                      |
| Coverage                     | `--cov/--cov-report`              | `--cov [--cov-merge-full]`                  | Incremental by default; fast combiner                             |
| Config file                  | `pytest.ini/pyproject.toml`       | `veri.toml` (or `pyproject.tool.veri`)    | Zero-config works; optional overrides                             |
| Plugins                      | `-p` / autoload                   | `--plugins allowlist` (default)             | Deterministic startup; `--engine pytest` escape hatch             |
| Full handoff                 | —                                 | `veri --engine pytest ...`                 | Use veri UX/reporters but run pure pytest when needed            |

---

# 2) Import-impact analysis (static, safe, fast)

## 2.1 Design goals

* **Instant** impacted set rebuild (<300 ms) after a save.
* **Static, conservative**: never miss tests → broaden selection when uncertain.
* **Deterministic**: the same inputs yield the same selection.

## 2.2 Project model (cached artifacts)

* `tests.index` — `{ file → [nodeids] }` from one initial collection.
* `module.map` — `{ file → dotted.module.name }` respecting packages (incl. PEP 420 namespaces).
* `imports.graph` — adjacency list `{ module → set[imported_module] }` from **AST only**.
* `revdeps.graph` — reverse deps computed once: `{ module → set[dependent_modules] }`.
* `fixtures.map` — `{ fixture_provider_file → subtree_paths }` (dir‐scoped via conftest rules).
* `markers.index` — `{ nodeid → set[markers] }`.
* `timings.json` — historical durations, `{ nodeid → mean_ms, p95_ms }`.

## 2.3 What we parse (AST rules)

* **Imports:** `import x[.y] as a`, `from x[.y] import z as b` → normalize to dotted names.
* **Relative imports:** resolve via `module.map` + package roots.
* **Dynamic imports:** detect `importlib.import_module`, `__import__`, `exec`, `eval` signatures:

  * If the arg is a constant string → treat as static import.
  * Else → mark the module as **dynamic**; add an “uncertain” edge to **ALL** package siblings (bounded fanout) or broaden selection to subtree.
* **Path surgery:** detect writes to `sys.path` / `PYTHONPATH` within tests/conftests → mark subtree **uncertain**.

## 2.4 Invalidation policy (when to broaden)

* Changed **test file** → run **its nodeids**.
* Changed **source file** `m.py` → run all tests whose transitive revdeps include `m`.
* Changed **conftest.py** in dir D → run tests under D (recursive).
* Changed **plugin list** or plugin versions → **full run** (or `--engine pytest` once).
* Changed **env keys** that affect import resolution (`PYTHONPATH`, `veri_IMPORT_MODE`) → broaden one level.
* Changed **interpreter/OS/lockfile** → drop caches (full re-collect once).
* **Uncertain signals** (dynamic import, path surgery, monkeypatch importers) → broaden to nearest safe package or full.

## 2.5 Safety valve

If impacted set > X% of suite (configurable, default 60%) or uncertain edges > Y per file (default 3), automatically switch to **mode=all** and tell the user:
“Impacted set exceeded threshold due to `dynamic import` in `src/loader.py`; running all tests (safe default).”

---

# 3) Scheduling & execution

* **Process pool** (per-test isolation) with sticky workers to avoid interpreter warm-up tax.
* **Order:** `last-failed` first → long poles (p95) front-loaded → rest balanced by historical **bin-packing**.
* **Concurrency lanes:** optional marker → lane mapping (e.g., `@db` = max 1 concurrent).
* **Fail-first:** show the first failure ASAP; continue in CI unless `--maxfail`.

---

# 4) Coverage (incremental by default)

* Collect coverage only for selected tests; maintain a **union map** on branch.
* **Fast combine:** instead of `coverage combine`, veri merges per-file bitsets keyed by digest; only changed files recompute.
* Flags:

  * `--cov` → turn on incremental coverage; emit XML/HTML if paths configured.
  * `--cov-merge-full` → after a selective CI run, read historical maps + run a short “sweep” for still-unseen files to produce a full report.
  * `--cov-diff-threshold=80` → gate changed lines (PR focus).

---

# 5) Output & data contracts

## 5.1 JSONL event stream (stable)

Each line is one JSON event (ordered):

```json
{"t":"start","run_id":"2025-08-27T10:41:03Z","engine":"veri","workers":8}
{"t":"plan","changed":["src/parser.py"],"impacted":["tests/test_parse.py::test_a","tests/test_parse.py::test_b"]}
{"t":"case","nodeid":"tests/test_parse.py::test_a","status":"pass","ms":18}
{"t":"case","nodeid":"tests/test_parse.py::test_b","status":"fail","ms":41,"error":"AssertionError ..."}
{"t":"summary","passed":128,"failed":1,"skipped":4,"duration_ms":6123}
```

## 5.2 Shard manifest (CI)

```json
{
  "schema": "veri-shards@1",
  "generated_at": "2025-08-27T10:41:03Z",
  "strategy": "timings+binpack",
  "workers": 4,
  "shards": [
    {"id": 0, "nodeids": ["tests/a::t1", "tests/c::t3"], "est_ms": 31000},
    {"id": 1, "nodeids": ["tests/b::t2", "tests/b::t4"], "est_ms": 30500},
    {"id": 2, "nodeids": ["tests/d::t1"], "est_ms": 30050},
    {"id": 3, "nodeids": ["tests/e::t*"], "est_ms": 29900}
  ]
}
```

---

# 6) CI replacements (one binary → many plugins gone)

## 6.1 GitHub Actions (Linux + Windows, `uv`, cache veri)

```yaml
name: tests
on: [push, pull_request]

jobs:
  test:
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest]
        shard: [0,1,2,3]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4

      - name: Install uv + veri
        run: |
          pipx install uv
          uv tool install veri

      - name: Restore veri cache
        uses: actions/cache@v4
        with:
          path: .veri/cache
          key: ${{ runner.os }}-veri-${{ hashFiles('uv.lock', 'pyproject.toml') }}

      - name: Install deps
        run: uv sync --frozen

      - name: Create shards
        if: ${{ matrix.shard == 0 }}
        run: veri split --ci 4 > shards.json

      - name: Upload shards
        if: ${{ matrix.shard == 0 }}
        uses: actions/upload-artifact@v4
        with: { name: shards, path: shards.json }

      - name: Download shards
        if: ${{ matrix.shard != 0 }}
        uses: actions/download-artifact@v4
        with: { name: shards }

      - name: Run shard
        run: veri --ci shard --ci ${{ matrix.shard }} --junit-xml reports/junit.xml --cov --cov-merge-full

      - name: Upload reports
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: reports-${{ matrix.os }}-${{ matrix.shard }}
          path: reports
```

## 6.2 GitLab CI

```yaml
stages: [test]
variables: { PIPX_BIN_DIR: "$CI_PROJECT_DIR/.local/bin" }

test:
  stage: test
  parallel: 4
  script:
    - pipx install uv
    - uv tool install veri
    - uv sync --frozen
    - if [ "$CI_NODE_INDEX" = "1" ]; then veri split --ci 4 > shards.json; fi
    - '[ "$CI_NODE_INDEX" != "1" ] && cp ../0/shards.json . || true'
    - veri --ci shard --ci $((CI_NODE_INDEX-1)) --junit-xml reports/junit.xml --cov --cov-merge-full
  artifacts:
    when: always
    paths: [reports, .veri/cache]
```

## 6.3 Azure Pipelines

```yaml
pool: { vmImage: 'ubuntu-latest' }
strategy:
  parallel: 4
steps:
  - checkout: self
  - script: pipx install uv && uv tool install veri
    displayName: Install tools
  - script: uv sync --frozen
    displayName: Install deps
  - script: veri split --ci 4 > shards.json
    condition: eq(variables['System.JobPositionInPhase'], 1)
  - script: veri --ci shard --ci $(System.JobPositionInPhase) --junit-xml reports/junit.xml --cov --cov-merge-full
    displayName: Run shard
  - task: PublishTestResults@2
    inputs: { testResultsFormat: 'JUnit', testResultsFiles: 'reports/junit.xml', failTaskOnFailedTests: true }
```

**What this replaces**

* `pytest -n auto` (xdist), `pytest-split`, `pytest-watcher`, `pytest-cov combine`, and often `pytest-testmon`.

---

# 7) Plugin compatibility stance

| Plugin kind                                                | Status in v1 | Handling                                                        |
| ---------------------------------------------------------- | ------------ | --------------------------------------------------------------- |
| Core pytest semantics (fixtures, marks, `-k`, parametrize) | ✅            | Covered                                                         |
| Async & frameworks (`pytest-asyncio`, FastAPI, Django)     | ✅            | Execution in worker process; collection cached                  |
| xdist                                                      | Not needed   | Use `--workers` (built-in pool)                                 |
| Split/shard plugins                                        | Replaced     | `split/shard` built-in                                          |
| Coverage (`pytest-cov`)                                    | Replaced     | Native `--cov` (faster combine)                                 |
| Snapshot/approval tests                                    | ✅/⚠️         | Works; add `skein` later for smart diffs                        |
| Exotic collection-mutating plugins                         | ⚠️           | Detect and auto `--engine pytest` handoff with friendly message |

---

# 8) Error/diagnostic UX (examples)

* `veri --explain` after editing `src/parser.py`:

```
Changed files (1):
  src/parser.py

Impacted tests (2):
  tests/test_parse.py::test_a   (via import: src.parser -> tests.test_parse)
  tests/test_parse.py::test_b   (via import: src.parser -> src.lexer -> tests.test_parse)

Safety notes:
  No dynamic imports detected. Conftest unchanged.
```

* When broadening:

```
Detected dynamic import in src/loader.py (importlib.import_module(variable)).
Selection broadened to package 'src' (43 tests) for safety. Use --engine pytest to force full run.
```

---

# 9) Minimal internal algorithms (pseudocode)

## 9.1 Build module & import graph (one-time or cache miss)

```python
from pathlib import Path
import ast

def module_name(file: Path, roots: list[Path]) -> str:
    root = max((r for r in roots if file.is_relative_to(r)), key=lambda r: len(str(r)))
    rel = file.relative_to(root).with_suffix('')
    # handle __init__.py and PEP 420: drop trailing "__init__"
    parts = [p for p in rel.parts if p != "__init__"]
    return ".".join(parts)

def parse_imports(file: Path) -> set[tuple[str, bool]]:
    # returns {(dotted_name, is_dynamic)}
    src = file.read_text(encoding="utf-8", errors="ignore")
    try:
        tree = ast.parse(src, filename=str(file))
    except SyntaxError:
        return set()
    out: set[tuple[str,bool]] = set()
    for n in ast.walk(tree):
        if isinstance(n, ast.Import):
            for a in n.names:
                out.add((a.name, False))
        elif isinstance(n, ast.ImportFrom) and isinstance(n.module, str):
            out.add((("." * (n.level or 0)) + n.module, False))
        elif isinstance(n, ast.Call) and getattr(getattr(n.func, "attr", ""), "") in {"import_module", "__import__"}:
            # literal string → static; else dynamic
            arg = n.args[0] if n.args else None
            static = isinstance(arg, ast.Constant) and isinstance(arg.value, str)
            out.add(((arg.value if static else "*dynamic*"), not static))
    return out
```

## 9.2 Compute impacted tests on change

```python
def impacted_tests(changed_files: set[Path], graphs, indexes) -> set[str]:
    mods = {graphs.module_map[f] for f in changed_files if f.suffix == ".py"}
    # broaden for conftest changes
    if any(p.name == "conftest.py" for p in changed_files):
        return indexes.tests_under_dirs({p.parent for p in changed_files})
    # expand via reverse deps
    dep_mods = closure_revdeps(mods, graphs.revdeps)
    impacted_files = {graphs.file_map[m] for m in dep_mods if m in graphs.file_map}
    return set().union(*(indexes.tests_index[f] for f in impacted_files if f in indexes.tests_index))
```

---

# 10) Caching & keys

**Cache key tuple (hashed, printed in `--explain`):**
`(python_version, platform, veri_version, uv_lock_digest, site_packages_digest, pytest_version, plugins@versions, conftest_digests, veri_config_digest)`

* **Cold path:** miss → collect once via pytest; build indexes/graphs; warm timings as we go.
* **Hot path:** no collection; plan in memory; spawn workers immediately.

---

# 11) Bench methodology (repeatable)

* **Suites:** fastapi, pydantic, polars, sqlalchemy, your largest internal repo.
* **Runs:** (a) cold full run, (b) edit small source file → impacted run, (c) CI 4-shard full run.
* **Metrics:** wall-clock, CPU-sec, time-to-first-failure, stddev over 5 runs.
* **Targets:** ≥2× faster collection vs pytest; ≥20–30% CI wall-clock reduction; ≤300 ms watch loop on small edits.

---

# 12) Migration recipes

## 12.1 Local (devs)

* Keep `pytest.ini` as-is. Add optional `veri.toml` only if you need overrides.
* Start with:

  ```
  uv tool install veri
  veri -a            # first full run; warms caches
  veri -w            # impact-aware watch
  ```
* If a plugin misbehaves: `veri --engine pytest -k "expr"` for that session; report the plugin for the compat list.

## 12.2 CI (replace the stack)

* Replace: `pytest -n auto --cov ...` + `pytest-split` + coverage combine + watch plugins
* With: `veri --ci [split/shard] --cov --cov-merge-full --junit-xml ...`

---

# 13) Roadmap checkpoints that de-risk competition

* **v0.1 (MVP):** fast collector, AST graph, impacted runs, watch, timings, workers, JUnit, basic `--cov`.
* **v0.2:** stable sharding manifest; flaky auto-rerun(=1); `--explain`; Windows polish.
* **v0.3:** diff-coverage gate, lanes via markers; fast combiner; plugin compat catalog + auto handoff.
* **v1.0:** remote cache (opt-in), quarantine list, TUI.

---

# 14) What veri eliminates vs incumbents (quick matrix)

| Friction                  | Today                        | veri                                  |
| ------------------------- | ---------------------------- | -------------------------------------- |
| Per-worker collection     | xdist does it                | **One** collection feeds all workers   |
| Multi-machine sharding    | Third-party plugins          | Built-in, timing-aware manifest        |
| Changed-code runs         | testmon with runtime tracing | **Static graph**, instant, no tracing  |
| Coverage combine slowness | `coverage combine`           | Fast native combiner keyed by digest   |
| Watch mode                | Plugins rerun too much       | Impact-aware, sub-second               |
| Plugin unpredictability   | Autoload everything          | Allowlist; auto handoff to pure pytest |

---

If you want, I can package this into a short **RFC.md** (with rationale and alternatives) or sketch the **crate layout** (Rust core + thin Python worker) and the **`.veri/*` file schemas** so you can open issues/milestones from day one.
