# veri — End-to-End Implementation & Verification Plan (from attached specs)

Below is a complete, *phase-by-phase* plan to build **veri** (single-binary, pytest-compatible, impact-aware, ultra-fast test runner). Each step includes: what to build, how to verify it, artifacts to produce, and the “definition of done” (DoD). It stitches together the deliverables, algorithms, CLI surface, schemas, CI, performance benchmarks, security/telemetry, migration, and roadmap from your docs into an executable plan.&#x20;

---

## Phase 0 — Repo bootstrap & governance (1–2 days)

**0.1 Create mono-repo scaffold**

* Layout:

  ```
  /                        # mono-repo
    crates/
      veri-core/           # Rust: planner, scheduler, cache
      veri-cli/            # Rust: CLI, reporters, TUI
    py_worker/             # Python pytest shim
    schemas/               # JSON Schemas (stable contracts)
    ci/                    # CI templates (GH/GitLab/Azure)
    scripts/bench.py       # benchmark harness (later)
  ```

  Source of truth for this layout.&#x20;

**0.2 Toolchain & policy**

* Tools: `uv`, `maturin`, `ruff`, `mypy`, `cargo`, `clippy`. Add a `Makefile`/`justfile` for one-liners (`just dev`, `just test`).&#x20;
* License: Apache-2.0 (optionally dual with MIT). Add `CODEOWNERS`.&#x20;
* Branding: name/tagline/emoji/color for docs and CLI banner.&#x20;

**0.3 Verify / DoD**

* `uv tool install maturin ruff mypy` works and `cargo --version` prints. Dev env doc mirrors CONTRIBUTING.&#x20;
* CI runs “lint + build + unit” on Linux/macOS/Windows (hello-world binaries).
* LICENSE present (Apache-2.0).

---

## Phase 1 — CLI skeleton & configuration (3–4 days)

**1.1 Implement CLI surface (no business logic yet)**

* Commands/flags per SPEC: `veri [-a|--all] [-w|--watch] -k -m --workers --last-failed --junit-xml --jsonl --explain --engine pytest` and CI helpers `split --ci N` / `shard --ci I`. Exit codes 0–4.&#x20;
* Config lookup: `veri.toml` or `[tool.veri]` in `pyproject.toml`; env vars `veri_LOG`, `veri_NO_COLOR`, `veri_CACHE_DIR`.&#x20;

**1.2 Verify / DoD**

* `veri -h` shows the full flag set and exit codes match SPEC.&#x20;
* Config precedence: flag > config > default (unit test).
* `--version` prints semver and build info.

---

## Phase 2 — Cache contracts & schemas (2–3 days)

**2.1 Implement stable JSON Schemas & writers**

* Persist and validate:

  * `tests.index`, `module.map`, `imports.graph`, `revdeps.graph`, `fixtures.map`, `markers.index`, `timings.json`, `event.jsonl` (per line), `shards.manifest`.&#x20;
* Add schema tests (round-trip) and a CI job that validates artifacts generated during integration tests.&#x20;

**2.2 Cache keys**

* Hash tuple `(python, platform, veri_version, uv_lock_digest, site_packages_digest, pytest@ver, plugins@vers, conftest_digests, veri_config_digest)`. Print in `--explain`.&#x20;

**2.3 Verify / DoD**

* Schema validation passes for golden samples; invalid docs are rejected with clear messages.
* `--explain` prints cache key components exactly as in examples.&#x20;

---

## Phase 3 — Python worker shim (initial collection & execution) (4–5 days)

**3.1 Minimal pytest compatibility layer**

* First run: use pytest to **collect** nodeids/markers/fixtures; persist `tests.index` + `markers.index`. Subsequent runs: execute **by nodeid list** handed from planner.&#x20;
* `--engine pytest` flag: fully hand off to upstream pytest for a single run (escape hatch).&#x20;

**3.2 Verify / DoD**

* `veri -a` on a sample repo runs all tests and writes `tests.index`.
* `veri --engine pytest` faithfully mirrors pytest exit code and JUnit output paths.&#x20;

---

## Phase 4 — Static import graph & reverse-deps (6–8 days)

**4.1 AST parser (fast, conservative)**

* Parse `import`/`from` forms, resolve relatives with file→module map (PEP 420 aware). Detect dynamic imports (`importlib.import_module`, `__import__`) and mark as **uncertain**, widening selection as per rules.&#x20;
* Build `imports.graph`, `revdeps.graph`, `module.map`. Persist per schemas.&#x20;

**4.2 Invalidation & safety valves**

* Rules:

  * Test file changed → run its nodeids.
  * Source file changed → run tests whose revdeps include that module.
  * `conftest.py` change → run tests under that directory.
  * Plugin list/version change → full run (or `--engine pytest`).
  * Impacted set threshold (default 60%) → broaden to all; log reason.&#x20;

**4.3 Verify / DoD**

* Golden tests for import resolution (abs/rel, packages, namespace pkgs).
* E2E: edit `src/parser.py` → `--explain` shows impacted nodeids and reason chain; matches the example format.&#x20;
* Dynamic import detected → broaden with the friendly message.&#x20;

---

## Phase 5 — Planner, scheduler & workers (5–7 days)

**5.1 Planner (impacted set)**

* Implement the pseudocode precisely, including broaden-on-uncertainty & ratio threshold.&#x20;

**5.2 Scheduler (bin-pack + fail-first)**

* Ordering: last-failed first → long poles (p95) → rest by estimate; bin-pack across `--workers`. Concurrency lanes by markers later (Phase 8).&#x20;

**5.3 Process pool**

* Sticky worker processes for amortized interpreter warm-up; graceful cancellation on watch edits. Architecture contract per diagram.&#x20;

**5.4 Verify / DoD**

* Deterministic plans for same inputs; seed stability test.
* Scheduler unit tests show improved balance (no idle long tail) against synthetic timings.
* Pool keeps \<N processes and reuses them between tasks.

---

## Phase 6 — Coverage (incremental + fast combine) (4–6 days)

**6.1 Incremental coverage**

* Only measure for **selected** tests; maintain per-file union map keyed by digest; produce XML/HTML if configured.&#x20;

**6.2 Fast combiner**

* Replace `coverage combine` with digest-keyed merge of bitsets; add `--cov-merge-full` to produce a full report in CI.&#x20;

**6.3 Verify / DoD**

* Unit tests on merge-logic (idempotence, associativity).
* E2E: selective run followed by `--cov-merge-full` yields same totals as a full `-a` coverage run within tolerance.

---

## Phase 7 — Watch mode (impact-aware) (3–4 days)

**7.1 File watcher**

* Cross-platform debounce; Git-aware ignores; interrupts running batch on save. TUI: minimal progress + “impacted by …” hints.&#x20;

**7.2 Verify / DoD**

* Edit small src file: end-to-first-failure **≤300 ms** on a medium repo (target). Benchmark harness will verify.&#x20;

---

## Phase 8 — CI sharding & artifacts (3–5 days)

**8.1 Sharding**

* `split --ci N` → writes `veri-shards@1` manifest; `shard --ci I` consumes it. Strategy: timings + bin-pack; stable assignment.&#x20;
* Emit JUnit + JSONL event stream (`t: start|plan|case|summary|log`) for dashboards.&#x20;

**8.2 CI templates**

* Ship GitHub Actions, GitLab CI, Azure Pipelines examples using `uv tool install veri`, cache `.veri/cache`, run shards, upload artifacts.

**8.3 Verify / DoD**

* Matrix CI (Ubuntu + Windows) runs 4 shards with balanced durations (±10% spread).
* JUnit recognized by CI test publishers; JSONL validated against schema.

---

## Phase 9 — UX polish, diagnostics & error copy (2–3 days)

**9.1 `--explain` & messages**

* Implement “why these tests ran” (changed files, import path, safety notes). Include printed cache key tuple.&#x20;
* Great messages for common questions (“Why didn’t my test run?”, dynamic import broaden, per-worker collection avoided).&#x20;

**9.2 Verify / DoD**

* Golden snapshots for `--explain` output (stable, sorted).
* Error copy matches examples and links to docs.

---

## Phase 10 — Security & telemetry (2 days)

**10.1 Security posture**

* Default plugin **allowlist**; detect unsafe autoload; optional Sigstore for binary releases; sandbox guidance for workers (`--no-network` in runner itself).&#x20;

**10.2 Telemetry (opt-in only)**

* Minimal counters + timings (no nodeids/paths); global off by default; `--no-network` hard-blocks I/O.&#x20;

**10.3 Verify / DoD**

* “Telemetry OFF” confirmed by tests and by inspecting outbound calls (none).
* Security checklist in `SECURITY.md` complete.&#x20;

---

## Phase 11 — Docs & migration (2–3 days)

**11.1 Author top-tier docs**

* `README.md` quickstart (uv install, fast path), `RFC.md` rationale, `SPEC.md` for CLI/config/exits, `MIGRATION.md` from pytest, `CONTRIBUTING.md`, `BENCHPLAN.md`, `ROADMAP.md`, `ERROR-COPY.md`, `BRANDING.md`.&#x20;
* Competitive spec as an appendix (pytest ⇄ veri mapping; CI replacements).&#x20;

**11.2 Verify / DoD**

* Run docs examples verbatim: commands succeed; outputs match snippets.

---

## Phase 12 — Benchmarks & OKRs (4–6 days, repeated)

**12.1 Harness & suites**

* `scripts/bench.py` to run: cold full; hot impacted (small edit); CI 4-shard full. Suites: fastapi, pydantic, polars, sqlalchemy, one large internal repo. Metrics: wall-clock, CPU-sec, TTF-Failure, stddev (n=5). Targets: ≥2× faster collection than pytest; ≥20–30% CI wall-clock reduction; ≤300 ms watch rerun for small edits.&#x20;

**12.2 Verify / DoD**

* Publish JSON + summary Markdown per suite; deltas vs `pytest -n auto` baselines meet/approach targets.
* Non-regression perf CI (run weekly).

---

## Phase 13 — Beta hardening & plugin stance (ongoing, 1–2 weeks)

**13.1 Compatibility matrix**

* ✅ Core pytest semantics (fixtures, marks, parametrize), async frameworks; ⚠️ exotic collection-mutating plugins → auto fallback with guidance.&#x20;

**13.2 Flaky ergonomics (initial)**

* Auto re-run once on failure (N=1) and annotate JSONL. Queue advanced quarantine for v1.&#x20;

**13.3 Verify / DoD**

* Matrix tests: Py 3.9–3.13, Linux/macOS/Windows, with/without venv, uv/pip.
* Known incompatible plugins trigger `--engine pytest` handoff automatically with a friendly message.

---

## Phase 14 — v1 features & release criteria

**14.1 Features to close v1**

* Diff-coverage gate (`--cov-diff-threshold`), marker-based resource lanes, remote cache (opt-in), minimal TUI, quarantine list.&#x20;

**14.2 Release criteria (acceptance)**

* Functionality: All CLI endpoints work; schemas stable.
* Performance: Benchmarks hit P0/P1/P2 targets or have documented gaps + roadmap.&#x20;
* Reliability: Deterministic planning; no missed-test bugs under static analysis (safety broaden kicks in).&#x20;
* Docs: All primary docs complete and tested.
* Security/telemetry: defaults hardened (off/allowlist).&#x20;

---

## Cross-cutting checklists

### A) Implementation checklists (by area)

* **CLI & Config** — Flags implemented per SPEC; config precedence honored; exit codes 0–4.&#x20;
* **Schemas** — All 9 schemas exist and validate; CI job runs `jsonschema` on produced artifacts.&#x20;
* **Planner/Graph** — AST rules cover imports, relatives, dynamic patterns; invalidation rules match COMPETITIVE SPEC; threshold broaden works.&#x20;
* **Scheduler** — Uses historical timings (`timings.json`); bin-pack; fail-first.&#x20;
* **Coverage** — Incremental by default; fast combiner; `--cov-merge-full` in CI.&#x20;
* **CI** — GH/GitLab/Azure examples run; shards manifest `veri-shards@1` round-trips.&#x20;
* **Diagnostics** — `--explain` + excellent error copy messages implemented.&#x20;

### B) Verification playbook (quick commands)

* **Cold warm:** `veri -a --jsonl .veri/events.jsonl` → `tests.index` written; `start/plan/summary` events present.&#x20;
* **Hot impact:** edit `src/x.py`; run `veri --explain` → shows impacted nodeids via reverse-deps chain.&#x20;
* **Dynamic import safety:** add `importlib.import_module(variable)`; run `veri` → broaden message appears.&#x20;
* **Sharding:** `veri split --ci 4 > shards.json` then `veri shard --ci 2 --manifest shards.json` → runs that shard only; manifest validates.&#x20;
* **Coverage merge:** `veri --cov` → cache maps updated; CI: `--cov --cov-merge-full` produces full XML.&#x20;
* **Watch loop:** `veri -w` then edit a file → sub-second impacted re-run; TTF-Failure logged.&#x20;

---

## Delivery timeline (from your roadmap; adjust per team capacity)

* **Phase 0 (Spike)** — Rust CLI skeleton, Python worker, `tests.index`, nodeid execution, basic pool, watch: *1–2 weeks*.&#x20;
* **Phase 1 (MVP)** — AST graph + impacted planner, timings & scheduler, JUnit/JSONL, basic coverage, Windows parity, docs: *4–6 weeks*.&#x20;
* **Beta** — Sharding manifest, `--explain`, flaky auto-rerun(=1), minimal TUI, plugin compat catalog: *then iterate*.&#x20;
* **v1** — Diff-coverage gate, marker lanes, fast combiner, remote cache (opt-in), quarantine list: *finalize*.&#x20;

---

## Risk register & mitigations

* **Missed tests due to under-approximation** → Strictly conservative static analysis; broaden when uncertain; nightly full sweep in CI recommended.&#x20;
* **Plugin edge cases** → Auto detect & `--engine pytest` fallback with friendly guidance; maintain a living compat matrix.&#x20;
* **Coverage perf** → Keep incremental default; only `--cov-merge-full` in CI; verify combine parity against full run periodically.&#x20;
* **Windows quirks** → PowerShell-friendly defaults; disable ANSI by default; parity tests in CI.&#x20;

---

## “Definition of Done” (project-level)

1. **Correctness:** For any edit, either the minimal impacted tests run or selection is safely broadened; never silently misses tests. Evidence: targeted test suites + broaden policies exercised.&#x20;
2. **Performance:** Meets/approaches targets from BENCHPLAN on chosen suites; perf regressions tracked in CI.&#x20;
3. **DX & CI:** One-liner quickstart + ready-to-paste CI templates; JUnit/coverage/JSONL artifacts integrate in standard tools.&#x20;
4. **Security & Trust:** Allowlist plugins; telemetry off by default; `--no-network` respected; SECURITY/TELEMETRY docs complete.&#x20;
5. **Docs:** README, RFC, SPEC, MIGRATION, BENCHPLAN, ROADMAP, ERROR-COPY, BRANDING are present and tested.&#x20;

---

### Appendix — Competitive & pain-point alignment checks

Before each milestone, sanity-check that veri still solves the concrete pains (per-worker collection overhead, CI imbalance, slow coverage combine, coarse watch) and remains a single-binary drop-in replacing xdist/split/testmon/cov-combine in CI. Use the side-by-side flags table and CI replacement templates to confirm parity.

---

If you want, I can turn this into a GitHub Project with issues/milestones (one ticket per step above, each with the verification checklist and DoD included).
