## Quick orientation (Phase 0 - Rust/Python Hybrid)

This is a **hybrid Rust/Python monorepo** implementing the veri test runner. Focus areas:

- **Rust workspace**: `crates/veri-core` (planner/scheduler) + `crates/veri-cli` (CLI binary)
- **Python worker**: `py_worker/` (pytest shim for test execution)
- **Infrastructure**: `schemas/` (JSON contracts), `ci/` (GitHub Actions), `docs/` (implementation plan)
- **Reference pytest**: `pytest/` (vendored, read-only examples and test patterns)

Keep changes focused on the Rust crates and Python worker; treat `pytest/` as documentation only.

## Architecture (Phase 0 status)

- **Rust CLI**: `crates/veri-cli/src/main.rs` - placeholder binary (will become full CLI in Phase 1)
- **Rust Core**: `crates/veri-core/src/lib.rs` - core logic library (planning/scheduling/caching)
- **Python Worker**: `py_worker/veri_worker.py` - pytest compatibility shim (will execute tests)
- **Workspace**: `Cargo.toml` - Rust workspace config with shared metadata
- **Build System**: `justfile`/`Makefile` - cross-platform build automation
- **CI**: `.github/workflows/` + `ci/github-actions.yml` - multi-platform validation

## Developer workflows (Phase 0)

**Primary toolchain**: Rust (`cargo`) + Python (`uv`) + build automation (`just` or `make`)

```powershell
# Setup (one-time)
just setup     # installs uv, ruff, mypy, maturin, cargo-nextest
# OR: make setup

# Daily development
just check     # format + lint + build + test everything
# OR: make check

# Individual commands
just dev       # cargo build --workspace
just test      # cargo test --workspace  
just fmt       # cargo fmt --all
just lint      # cargo clippy --workspace --all-targets
```

## Where to look (Phase 0)

- **`Cargo.toml`** — workspace config, shared metadata
- **`crates/veri-cli/Cargo.toml`** — CLI binary dependencies
- **`crates/veri-core/Cargo.toml`** — core library dependencies  
- **`py_worker/pyproject.toml`** — Python worker packaging
- **`justfile`** / **`Makefile`** — build commands
- **`ci/github-actions.yml`** — CI template
- **`docs/IMPLEMENTATION_PLAN.md`** — phase-by-phase roadmap
- **`docs/BRANDING.md`** — project identity (⚡ lightning emoji, colors)

## Editing guidance (Phase 0)

- **Rust changes**: Edit `crates/veri-*/src/` files, run `just check` to validate
- **Python worker**: Edit `py_worker/veri_worker.py`, ensure it stays pytest-compatible
- **Don't edit `pytest/`** - it's a vendored reference for patterns/examples
- **Build/CI changes**: Update `justfile`/`Makefile` and `ci/github-actions.yml`
- **New dependencies**: Add to appropriate `Cargo.toml` or `py_worker/pyproject.toml`
- **Always run `just check`** before committing to ensure format/lint/build/test pass
## Phase 0 Verification Commands

```powershell
# Verify Phase 0 DoD (Definition of Done)
cargo --version                    # ✅ Rust toolchain available
cargo build --workspace           # ✅ Workspace builds successfully  
cargo test --workspace            # ✅ All tests pass
just check                        # ✅ Format, lint, build, test pass
# OR: make check

# CI validation (locally)
cargo fmt --all -- --check        # ✅ Code is formatted
cargo clippy --workspace --all-targets -- -D warnings  # ✅ No lint warnings


