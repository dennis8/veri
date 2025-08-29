.PHONY: build test fmt lint check clean setup

# Development build
build:
	cargo build --workspace

# Run tests
test:
	cargo test --workspace

# Format code
fmt:
	cargo fmt --all

# Run clippy lints
lint:
	cargo clippy --workspace --all-targets -- -D warnings

# Check everything (format, lint, build, test)
check: fmt lint build test

# Clean build artifacts
clean:
	cargo clean

# Install dev tools
setup:
	@echo "Installing development tools..."
	pip install uv ruff mypy maturin
	cargo install --force cargo-nextest
