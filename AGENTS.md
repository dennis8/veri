# Repository Guidelines

## Project Structure & Module Organization
- Root workspace: Rust + Python mono-repo.
- `crates/`: Rust crates — `veri-core` (planner, scheduler, cache, worker) and `veri-cli` (CLI, reporters).
- `py_worker/`: Python pytest shim and integration layer (`veri_worker.py`, `tests/`).
- `schemas/`: JSON Schemas (stable contracts).
- `docs/`, `examples/`, `ci/`, `scripts/`: Reference, demos, CI, and tooling.

## Build, Test, and Development Commands
- Rust build/tests: `cargo build --workspace`, `cargo test --workspace`.
- Make targets: `make build`, `make test`, `make fmt`, `make lint`, `make check`.
- Just tasks (preferred): `just dev`, `just test`, `just fmt-all`, `just lint-all`, `just check`.
- Python tests: `cd py_worker && uv run pytest -v` or `just test-py`.
- Bootstrap tools: `just setup` (installs `cargo-nextest`, syncs Python deps with `uv`).

## Coding Style & Naming Conventions
- Rust: `rustfmt` defaults; lint with `clippy -D warnings`. Modules/files `snake_case`; types `PascalCase`; functions `snake_case`.
- Python: format with `ruff format .`; lint with `ruff check .`; type-check with `mypy .`. Modules/files `snake_case`; classes `PascalCase`; 4-space indents.
- Keep public APIs stable under `schemas/`; document breaking changes in `docs/MIGRATION.md`.

## Testing Guidelines
- Rust unit tests live alongside code as `mod tests { ... }`; CLI tests in `crates/veri-cli/src/cli_tests.rs`. Run subset: `cargo test scheduler`.
- Python tests under `py_worker/tests/` using `pytest`. Name files `test_*.py`, tests `test_*`.
- Run everything before pushing: `just check` or `make check`.

## Commit & Pull Request Guidelines
- Commits: prefer Conventional Commits (e.g., `feat(cli): add --watch mode`). Keep changes focused and atomic.
- PRs: include a clear description, linked issues, reproduction steps, and before/after notes (logs or screenshots for CLI output when relevant).
- Requirements: all checks green (`fmt`, `lint`, `build`, `test`), docs updated when behavior changes.

## Security & Configuration Tips
- Read `SECURITY.md` and `TELEMETRY.md` before shipping changes touching security/metrics.
- Do not commit secrets; respect `.gitignore` and local state under `.veri/`.
- User config lives in `veri.toml` (see `examples/veri.toml`); update `schemas/` if config shape changes.
