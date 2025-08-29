set shell := ['pwsh', '-c']

# Development build
dev:
	cargo build --workspace

# Run Rust tests
test:
	cargo test --workspace

# Run Python tests
test-py:
	cd py_worker && uv run pytest -v

# Run all tests
test-all: test test-py

# Format Rust code
fmt:
	cargo fmt --all

# Format Python code
fmt-py:
	cd py_worker && uv run ruff format .

# Format all code
fmt-all: fmt fmt-py

# Run Rust clippy lints
lint:
	cargo clippy --workspace --all-targets -- -D warnings

# Run Python lints
lint-py:
	cd py_worker && uv run ruff check .

# Run Python type checking
typecheck-py:
	cd py_worker && uv run mypy .

# Run all lints and type checks
lint-all: lint lint-py typecheck-py

# Check everything (format, lint, build, test)
check: fmt-all lint-all dev test-all

# Clean build artifacts
clean:
	cargo clean

# Install dev tools
setup:
	@echo "Installing development tools..."
	cargo install --force cargo-nextest
	cd py_worker && uv sync --dev
