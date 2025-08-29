# veri Test Runner — Comprehensive Artifact Pack v0.1

> Single-binary, pytest‑compatible, impact‑aware, ultra‑fast test runner.

This pack contains draft artefacts you can drop into a repo or turn into issues/milestones. Everything is **Astral‑grade**: fast path first, zero‑config by default, deterministic caching, PowerShell‑friendly.

---

## 0) Table of contents
- [README.md (Quickstart)](#readmemd-quickstart)
- [RFC.md (Rationale & Alternatives)](#rfcmd-rationale--alternatives)
- [SPEC.md (CLI, Config, Exit codes)](#specmd-cli-config-exit-codes)
- [ARCHITECTURE.md (Design & Algorithms)](#architecturemd-design--algorithms)
- [SCHEMAS/ (Stable file formats)](#schemas-stable-file-formats)
- [PSEUDOCODE.md (Core algorithms)](#pseudocodemd-core-algorithms)
- [CI/ (GitHub, GitLab, Azure)](#ci-github-gitlab-azure)
- [BENCHPLAN.md (Performance methodology)](#benchplanmd-performance-methodology)
- [ROADMAP.md (Delivery plan)](#roadmapmd-delivery-plan)
- [CONTRIBUTING.md (Dev guide)](#contributingmd-dev-guide)
- [SECURITY.md (Threat model)](#securitymd-threat-model)
- [TELEMETRY.md (Trust & privacy)](#telemetrymd-trust--privacy)
- [MIGRATION.md (From pytest)](#migrationmd-from-pytest)
- [ERROR-COPY.md (Great messages)](#error-copymd-great-messages)
- [BRANDING.md (Name, logo, tagline)](#brandingmd-name-logo-tagline)
- [LICENSE](#license)

---

## README.md (Quickstart)

### Why veri
- **Instant feedback**: run only impacted tests after each save.
- **One collection** feeds many workers (no per‑worker re‑collect).
- **First‑class CI**: timing‑aware sharding + JUnit + incremental coverage.
- **Zero‑config**: sensible defaults; optional `veri.toml`.

### Install
```powershell
# Windows/PowerShell
pipx install uv
uv tool install veri
```
```bash
# macOS/Linux
pipx install uv
uv tool install veri
```

### Fast path
```bash
veri            # impacted tests (default), fail-first
veri -w         # watch mode, sub-second reruns
veri -a         # run all tests (like pytest)
veri -k "expr"  # filter expression
veri --cov      # incremental coverage; add --cov-merge-full in CI
```

### Minimal config (optional)
`veri.toml`
```toml
[run]
workers = "auto"
fail_first = true

[selection]
mode = "impacted"   # changed|impacted|all
exclude = ["tests/slow/**"]

[ci]
junit_xml = "reports/junit.xml"
coverage_xml = "reports/coverage.xml"

[caching]
dir = ".veri/cache"
key_env = ["UV_LOCKHASH", "PYTHONPATH"]
```

---

## RFC.md (Rationale & Alternatives)

### Problem
Python teams burn minutes per edit on **collection**, **over‑broad reruns**, and **CI imbalance**. Plugins help (xdist, split, testmon, watcher, cov) but add setup cost and duplicate work.

### Goals
- **G1**: Hot edit → test results in **≤300 ms** on medium repos.
- **G2**: CI wall‑clock **−30% median** via timing‑aware sharding & selective reruns on PRs.
- **G3**: Deterministic, debuggable plans with `--explain`.
- **G4**: Windows/macOS/Linux parity; PowerShell‑friendly.

### Non‑Goals (v1)
- Full emulation of exotic pytest plugins that mutate collection arbitrarily.
- Remote execution farm.

### Why not “just pytest + plugins”
- **Per‑worker collection** in xdist multiplies startup.
- **Coverage combine** is slow; testmon impact is runtime‑tracing‑based.
- Multi‑machine sharding requires third‑party glue & durations files.

### Alternatives considered
- Pants/Bazel: very fast but steep adoption/BUILD metadata.
- Pure coverage‑driven impact: accurate but adds substantial overhead.

---

## SPEC.md (CLI, Config, Exit codes)

### CLI overview
```
veri [OPTIONS] [PATHS...]
```

#### Common
- `-a, --all` – run entire suite using veri collector
- `-w, --watch` – watch files and rerun impacted tests
- `-k EXPR` – filter tests by expression
- `-m EXPR` – marker expression filter
- `-x, --maxfail N` – stop after N failures (default: fail‑first in watch)
- `--workers N|auto` – parallel workers (default: auto)
- `--last-failed` – run last failures first
- `--junit-xml PATH` – write JUnit XML
- `--jsonl PATH` – write event stream JSONL
- `--explain` – print selection and cache keys
- `--engine pytest` – hand off execution to upstream pytest for one run

#### CI & sharding
- `split --ci N` – produce a stable N‑way shard manifest
- `shard --ci I [--manifest shards.json]` – run shard _I_

#### Coverage
- `--cov [PKG|PATH ...]` – measure incremental coverage
- `--cov-merge-full` – produce full report by merging cached maps
- `--cov-fail-under PCT` – fail if global coverage < PCT
- `--cov-diff-threshold PCT` – changed‑lines threshold gate

#### Help & version
- `-v, --version` – print version
- `-h, --help`

### Config locations
- `veri.toml`, or `[tool.veri]` in `pyproject.toml`. CLI flags override config.

#### `pyproject.toml` example
```toml
[tool.veri.run]
workers = 8

[tool.veri.ci]
junit_xml = "reports/junit.xml"
```

### Environment variables
- `veri_LOG` = `error|warn|info|debug|trace`
- `veri_NO_COLOR` = `1` to disable ANSI
- `veri_CACHE_DIR` overrides cache dir

### Exit codes
- `0` success (all selected tests passed)
- `1` test failures
- `2` usage error (bad flags/config)
- `3` internal error
- `4` no tests collected (consider `--all`)

---

## ARCHITECTURE.md (Design & Algorithms)

### Components
```
+-----------------------------+
| veri CLI (Rust)            |
|  - FS scan & debounce       |
|  - AST parse (imports)      |
|  - Planner (impacted set)   |
|  - Scheduler (timings)      |
|  - Reporters (TUI/JUnit)    |
+---------------+-------------+
                |
                v
+-----------------------------+
| Python Worker (thin)        |
|  - pytest compat shim       |
|  - execute nodeids          |
|  - coverage hooks           |
+-----------------------------+
                |
                v
+-----------------------------+
| .veri/ cache               |
|  - tests.index              |
|  - module.map               |
|  - imports.graph + revdeps  |
|  - fixtures.map, markers    |
|  - timings.json             |
|  - coverage.map             |
+-----------------------------+
```

### Caching keys (content‑addressed)
- `(python, platform, veri_version, uv_lock_digest, site_packages_digest, pytest@ver, plugins@vers, conftest_digests, veri_config_digest)`

### Invalidation rules (safe by default)
- Test file changed → run its nodeids
- Source file changed → run tests whose revdeps include module
- `conftest.py` changed in dir → run tests under dir
- Plugin list/versions changed → full run (or `--engine pytest`)
- Dynamic import/path surgery detected → broaden to nearest package / full

---

## SCHEMAS/ (Stable file formats)

> JSON Schemas (Draft 7). These enable validation and typed tooling.

### `schemas/tests.index.schema.json`
```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "tests.index",
  "type": "object",
  "patternProperties": {
    ".+": { "type": "array", "items": { "type": "string" } }
  },
  "additionalProperties": false
}
```

### `schemas/module.map.schema.json`
```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "module.map",
  "type": "object",
  "properties": {
    "files": {"type": "object", "patternProperties": {".+": {"type": "string"}}},
    "modules": {"type": "object", "patternProperties": {"^[a-zA-Z0-9_.]+$": {"type": "string"}}}
  },
  "required": ["files", "modules"],
  "additionalProperties": false
}
```

### `schemas/imports.graph.schema.json`
```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "imports.graph",
  "type": "object",
  "properties": {
    "edges": {
      "type": "array",
      "items": {"type": "array", "items": [{"type": "string"}, {"type": "string"}]}
    },
    "dynamic_modules": {"type": "array", "items": {"type": "string"}}
  },
  "required": ["edges"],
  "additionalProperties": false
}
```

### `schemas/revdeps.graph.schema.json`
```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "revdeps.graph",
  "type": "object",
  "properties": {
    "edges": {
      "type": "array",
      "items": {"type": "array", "items": [{"type": "string"}, {"type": "string"}]}
    }
  },
  "required": ["edges"],
  "additionalProperties": false
}
```

### `schemas/fixtures.map.schema.json`
```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "fixtures.map",
  "type": "array",
  "items": {
    "type": "object",
    "properties": {
      "file": {"type": "string"},
      "scope": {"type": "string", "enum": ["session", "package", "module", "class", "function"]},
      "paths": {"type": "array", "items": {"type": "string"}}
    },
    "required": ["file", "scope", "paths"],
    "additionalProperties": false
  }
}
```

### `schemas/markers.index.schema.json`
```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "markers.index",
  "type": "object",
  "patternProperties": {
    ".+": {"type": "array", "items": {"type": "string"}}
  },
  "additionalProperties": false
}
```

### `schemas/timings.json.schema.json`
```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "timings.json",
  "type": "object",
  "additionalProperties": {
    "type": "object",
    "properties": {
      "mean_ms": {"type": "number"},
      "p95_ms": {"type": "number"},
      "last_ms": {"type": "number"},
      "count": {"type": "integer", "minimum": 1}
    },
    "required": ["mean_ms", "p95_ms", "count"],
    "additionalProperties": false
  }
}
```

### `schemas/event.jsonl.schema.json`
```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "event.jsonl (per line)",
  "type": "object",
  "properties": {
    "t": {"type": "string", "enum": ["start", "plan", "case", "summary", "log"]},
    "run_id": {"type": "string"},
    "engine": {"type": "string"},
    "workers": {"type": "integer"},
    "changed": {"type": "array", "items": {"type": "string"}},
    "impacted": {"type": "array", "items": {"type": "string"}},
    "nodeid": {"type": "string"},
    "status": {"type": "string", "enum": ["pass", "fail", "skip", "xpass", "xfail", "error"]},
    "ms": {"type": "number"},
    "error": {"type": "string"},
    "passed": {"type": "integer"},
    "failed": {"type": "integer"},
    "skipped": {"type": "integer"},
    "duration_ms": {"type": "number"}
  },
  "required": ["t"],
  "additionalProperties": true
}
```

### `schemas/shards.manifest.schema.json`
```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "veri-shards@1",
  "type": "object",
  "properties": {
    "schema": {"type": "string", "const": "veri-shards@1"},
    "generated_at": {"type": "string"},
    "strategy": {"type": "string"},
    "workers": {"type": "integer", "minimum": 1},
    "shards": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "id": {"type": "integer"},
          "nodeids": {"type": "array", "items": {"type": "string"}},
          "est_ms": {"type": "number"}
        },
        "required": ["id", "nodeids"]
      }
    }
  },
  "required": ["schema", "generated_at", "workers", "shards"],
  "additionalProperties": false
}
```

---

## PSEUDOCODE.md (Core algorithms)

### Planner (impacted set)
```pseudo
on_change(files):
  if any is conftest.py:
    return tests_under_dirs(parents(files))
  mods = { module_of(f) for f in files if f.endswith('.py') }
  rev = closure(mods, revdeps_graph)
  impacted_files = { file_of(m) for m in rev if m in file_map }
  impacted = union(tests_index[f] for f in impacted_files)
  if dynamic_flags_touched or impacted_ratio > 0.6:
    return ALL_TESTS
  return impacted
```

### Scheduler (bin‑pack with fail‑first)
```pseudo
order = last_failed_first + long_poles_first + rest_by_estimate
buckets = binpack(order, workers, cost=node_time)
execute_in_parallel(buckets, lanes=marker_limits)
```

### Fast coverage combiner (per file)
```pseudo
for file in changed_files:
  base = coverage_map[file]  # bitset of lines
  delta = run_coverage_for_selected_tests(file)
  coverage_map[file] = base OR delta
```

---

## CI/ (GitHub, GitLab, Azure)

### GitHub Actions (Linux + Windows)
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
      - name: Install tools
        run: pipx install uv && uv tool install veri
      - name: Restore cache
        uses: actions/cache@v4
        with:
          path: .veri/cache
          key: ${{ runner.os }}-veri-${{ hashFiles('uv.lock', 'pyproject.toml') }}
      - name: Install deps
        run: uv sync --frozen
      - name: Create shards
        if: ${{ matrix.shard == 0 }}
        run: veri split --ci 4 > shards.json
      - uses: actions/upload-artifact@v4
        if: ${{ matrix.shard == 0 }}
        with: { name: shards, path: shards.json }
      - uses: actions/download-artifact@v4
        if: ${{ matrix.shard != 0 }}
        with: { name: shards }
      - name: Run shard
        run: veri --ci shard --ci ${{ matrix.shard }} --junit-xml reports/junit.xml --cov --cov-merge-full
      - uses: actions/upload-artifact@v4
        if: always()
        with: { name: reports-${{ matrix.os }}-${{ matrix.shard }}, path: reports }
```

### GitLab CI
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
  artifacts: { when: always, paths: [reports, .veri/cache] }
```

### Azure Pipelines
```yaml
pool: { vmImage: 'ubuntu-latest' }
strategy: { parallel: 4 }
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

---

## BENCHPLAN.md (Performance methodology)

- **Suites**: fastapi, pydantic, polars, sqlalchemy, 1 internal large repo.
- **Runs**: cold full; hot impacted (edit small src file); CI 4‑shard full.
- **Metrics**: wall‑clock, CPU‑sec, time‑to‑first‑failure, stddev (n=5).
- **Targets**: ≥2× faster collection vs pytest; ≥20–30% CI wall‑clock reduction; ≤300 ms watch rerun on small edits.
- **Harness**: `scripts/bench.py` emits JSON; compare to `pytest -n auto` baseline.

---

## ROADMAP.md (Delivery plan)

**Phase 0 – Spike (1–2 wks)**  
Rust CLI skeleton; Python worker; persist `tests.index`; run by nodeid; basic pool; watch.

**Phase 1 – MVP (4–6 wks)**  
AST graph; impacted planner; timings; JUnit/JSONL; basic coverage; Windows parity; docs.

**Phase 2 – Beta**  
Stable sharding; `--explain`; flaky auto‑rerun(=1); TUI minimal; plugin compat catalog.

**Phase 3 – v1**  
Diff‑coverage gates; lanes by markers; fast combiner; remote cache (opt‑in); quarantine list.

---

## CONTRIBUTING.md (Dev guide)

### Repo layout (proposed)
```
/                               # mono-repo
  crates/
    veri-core/                 # Rust: planner, scheduler, cache
    veri-cli/                  # Rust: CLI, reporters, TUI
  py_worker/                    # Python shim (minimal)
  schemas/                      # JSON schemas
  ci/                           # CI templates
```

### Dev setup
```bash
uv tool install maturin
uv tool install ruff
uv tool install mypy
uv sync
```

- `cargo test` (Rust), `uv run pytest` (Python shim)
- `ruff check` for Python; `cargo clippy` for Rust

### PR checklist
- Add/extend JSON Schemas if artefacts change
- Update `--explain` output example in README
- Add timings to `timings.json` fixtures for tests

---

## SECURITY.md (Threat model)

- **Supply chain**: verify plugin lists; default allowlist (no autoload). Optional Sigstore for release binaries.
- **Sandboxing**: tests execute untrusted code; respect `--workers` isolation; no network by default unless tests require.
- **Cache poisoning**: all cache artefacts are content‑addressed by digest; fail closed on mismatch.

---

## TELEMETRY.md (Trust & privacy)

- **Off by default.**
- When enabled: minimal counters + timings; no nodeids, no file paths.
- `--no-network` hard blocks network I/O from veri itself.

---

## MIGRATION.md (From pytest)

- Keep `pytest.ini`; add `veri.toml` only if needed.
- Start with `veri -a` to warm caches, then `veri -w`.
- Replace in CI: `pytest -n auto` + `pytest-split` + `pytest-cov` combine → `veri --ci shard ... --cov --cov-merge-full`.
- If a plugin misbehaves, use `veri --engine pytest` for that run.

---

## ERROR-COPY.md (Great messages)

- **Why didn’t my test run?**  
  `No impacted tests detected for src/parser.py (no reverse deps). Try --all or edit a test file to trigger watch.`

- **Broadened for safety**  
  `Dynamic import detected in src/loader.py (importlib.import_module with non-constant). Broadened selection to package 'src'. Use --engine pytest to force a full run once.`

- **Per-worker collection avoided**  
  `Collected 3,142 tests once in 1.2s; reused for 8 workers (saved ~6.7s).`

- **Explain**  
  `Cache key: py=3.12.4; os=win32; uv_lock=9d3a..; pytest=7.4.4; plugins=[xdist@3.5.0]; conftest=[tests/conftest.py@e1b2..]`

---

## BRANDING.md (Name, logo, tagline)

- **Name**: veri
- **Tagline**: *Run only what matters, immediately.*
- **Emoji**: ⚡
- **Color**: neutral (no ANSI by default; opt‑in with `--color`)

---

## LICENSE

- Default recommendation: **Apache‑2.0** (business‑friendly, patent grant).  
- Dual‑license option with **MIT** if you want maximum OSS adoption; keep team features (remote cache/quarantine) behind a permissive commercial add‑on.

