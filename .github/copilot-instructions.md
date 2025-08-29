## Quick orientation

This repository provides a small `veri` package (entry point `veri:main`) and a large vendored copy of the pytest test-suite under the `pytest/` folder. An AI agent should focus on two areas first:

- the root package: `src/veri` and `pyproject.toml` (packaging metadata and entry point)
- the vendored pytest subtree: `pytest/` (extensive examples, tests, internal APIs under `pytest/src/_pytest`)

Keep guidance short and actionable: reference these files rather than giving generic Python advice.

## Big-picture architecture (what to know)

- Root package: `src/veri/__init__.py` contains the package entrypoint `main()` (installed as `veri = "veri:main"` in `pyproject.toml`). It's intentionally minimal in this snapshot.
- Build system: the project uses PEP 517/518 with a custom build-backend `uv_build` (see `pyproject.toml`). Before changing packaging, inspect `pyproject.toml` rather than assuming setuptools or poetry.
- Vendored pytest subtree: `pytest/` is a self-contained test/documentation ecosystem. It contains its own `pyproject.toml`, `tox.ini`, `scripts/`, `doc/` and a large `testing/` folder. Many project examples and behaviours are implemented and tested here — treat it as a canonical source of patterns.

## Developer workflows and quick commands (uv-first)

- Tooling & install: this repository expects `uv` as the tool/package manager. Use `uv` to install CLI tools and to materialize project dependencies from `uv.lock` / `pyproject.toml`.
## Quick orientation

This repo contains a tiny root package `src/veri` (entry point `veri:main`) and a vendored copy of pytest under `pytest/` used as a read-only reference and testbed.

Keep changes scoped to `src/veri` and adjacent integration tests; treat `pytest/` as documentation/examples only (do not edit).

## Essentials

- Entry point: `veri = "veri:main"` in `pyproject.toml`.
- Build backend: `uv_build` (see `[build-system]` in `pyproject.toml`).
- Python requirement: >= 3.13 (root `pyproject.toml`).
- Package manager: prefer `uv` and `uv.lock` for reproducible installs.

## Quickstart (uv-first, PowerShell)

```powershell
pipx install uv
uv tool install veri
uv sync --frozen
veri        # impacted tests by default
veri -w     # watch
veri -a     # run all
veri split --ci 4 > shards.json
veri shard --ci 1 --manifest shards.json
```

## Where to look (fast)

- `pyproject.toml` — packaging, entry points, build-backend
- `uv.lock` — canonical lock used by CI (`uv sync --frozen`)
- `src/veri/__init__.py` — current entrypoint
- `pytest/` — vendored reference (read-only)

## Editing guidance (short)

- Don't edit `pytest/`. Add new integration tests under `src/veri` or a new top-level `testing/`/`pytest-integration/` folder and explain in the PR.
- For CLI / public API changes: update `pyproject.toml` entry points and add a small runnable example or test; run `uv sync --frozen` then `veri` locally to validate.
- When changing packaging or deps: update `pyproject.toml` and regenerate `uv.lock` via your `uv` workflow; CI should run `uv sync --frozen`.

If you'd like this shortened further (20 lines) or want a tiny PR checklist added, tell me which bits to trim or include.

