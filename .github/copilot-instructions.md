## Quick orientation - veri test runner (Phase 3 Complete)

**Hybrid Rust/Python monorepo** implementing the veri test runner:
- **Rust workspace**: `crates/veri-core` (planner/scheduler) + `crates/veri-cli` (CLI binary)  
- **Python worker**: `py_worker/` (pytest shim) - **ALWAYS USE UV**
- **Infrastructure**: `schemas/` (JSON contracts), `docs/` (implementation plan)
- **Reference**: `pytest/` (vendored, read-only examples)

Keep changes focused on Rust crates and Python worker; treat `pytest/` as documentation only.

## Architecture

- **CLI**: `crates/veri-cli/src/main.rs` - full CLI with engine selection and test filtering
- **Core**: `crates/veri-core/src/lib.rs` - core logic with Python worker integration
- **Worker**: `py_worker/veri_worker.py` - pytest compatibility layer
- **Build**: `justfile`/`Makefile` - cross-platform automation

## Critical: UV Usage ⚡

**NEVER USE PIP OR DIRECT PYTHON CALLS - ALWAYS USE UV**

```powershell
# Dependencies
cd py_worker && uv sync              # Install dependencies  
cd py_worker && uv add package-name  # Add new dependency

# Execution  
uv run --project py_worker -m veri_worker collect --work-dir .
uv run --project py_worker pytest
cd py_worker && uv run mypy veri_worker.py
```

**In Rust code - use UV subprocess calls:**
```rust
// ✅ CORRECT
let mut cmd = Command::new("uv");
cmd.arg("run").arg("--project").arg(&py_worker_path);

// ❌ WRONG  
let mut cmd = Command::new("python");  // DON'T DO THIS
```

## Developer workflows

```powershell
# Setup (one-time)
just setup     # installs uv, ruff, mypy, cargo-nextest

# Daily development
just check     # format + lint + build + test everything  

# Individual commands
just dev       # cargo build --workspace
just test      # cargo test --workspace
just fmt       # cargo fmt --all  
just lint      # cargo clippy --workspace --all-targets
```

## Key files

- **`crates/veri-cli/src/main.rs`** — CLI with engine selection, filtering
- **`crates/veri-core/src/python_worker.rs`** — Python subprocess management  
- **`py_worker/veri_worker.py`** — pytest compatibility layer
- **`py_worker/pyproject.toml`** — Python dependencies (uv managed)

## Editing guidance

- **Rust**: Edit `crates/veri-*/src/`, run `just check` to validate
- **Python**: Edit `py_worker/veri_worker.py`, test with `uv run`
- **Dependencies**: Use `uv add package-name` (never pip)
- **Testing**: Use `examples/phase3_demo/` for integration tests
- **Don't edit `pytest/`** - vendored reference only
- **Always run `just check`** before committing

## Verification (Phase 3)

```powershell
cd examples/phase3_demo
../../target/debug/veri-cli.exe --all           # ✅ Collect 18 tests
../../target/debug/veri-cli.exe -k addition     # ✅ Filter 6 tests
../../target/debug/veri-cli.exe --engine pytest # ✅ Hand off to pytest
../../target/debug/veri-cli.exe --explain       # ✅ Show cache keys

# Verify cache files
ls .veri/cache/tests.index.json    # ✅ Test metadata
ls .veri/cache/markers.index.json  # ✅ Marker data

# Direct worker testing
cd ../../
uv run --project py_worker -m veri_worker collect --work-dir examples/phase3_demo
```

