# Contributing to veri ⚡

## Development Environment Setup

### Prerequisites
- Rust (latest stable): https://rustup.rs/
- Python 3.11+: https://python.org/downloads/
- Git

### Quick Setup
```bash
# Clone the repository
git clone https://github.com/dennis8/veri.git
cd veri

# Install development tools (optional but recommended)
just setup
# OR
make setup

# Build and test
just check
# OR
make check
```

### Development Commands
```bash
# Build the workspace
just dev        # or make build

# Run tests
just test       # or make test

# Format code
just fmt        # or make fmt

# Run lints
just lint       # or make lint

# Check everything
just check      # or make check

# Clean artifacts
just clean      # or make clean
```

### Project Structure
```
/                        # mono-repo root
  crates/
    veri-core/           # Rust: planner, scheduler, cache
    veri-cli/            # Rust: CLI, reporters, TUI
  py_worker/             # Python pytest shim
  schemas/               # JSON Schemas (stable contracts)
  ci/                    # CI templates
  scripts/bench.py       # benchmark harness
  docs/                  # Documentation
```

### Making Changes
1. Create a feature branch
2. Make your changes
3. Run `just check` to ensure everything passes
4. Commit with clear messages
5. Push and create a pull request

### Code Style
- Rust: Follow `rustfmt` defaults and `clippy` suggestions
- Python: Follow `ruff` and `mypy` recommendations
- Commits: Use conventional commit format when possible