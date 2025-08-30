# veri ⚡

**Ultra-fast, pytest-compatible, impact-aware test runner**

veri is a single-binary test runner that makes your test suite faster by only running tests affected by your changes. It's a drop-in replacement for pytest with intelligent test selection, parallel execution, and CI-optimized sharding.

## ✨ Key Features

- **🚀 Impact-aware testing**: Only run tests affected by code changes
- **⚡ Ultra-fast collection**: Single collection phase, shared across workers  
- **🔄 Smart watch mode**: Sub-300ms feedback loop on file changes
- **📊 Intelligent sharding**: Timing-aware CI parallelization
- **🎯 Drop-in compatibility**: Works with existing pytest tests and configs
- **🔧 Single binary**: No complex plugin dependencies

## 🚀 Quick Start

### Installation

```bash
# Install via uv (recommended)
uv tool install veri

# Or via pip
pip install veri
```

### Basic Usage

```bash
# Run all tests (first time - builds cache)
veri -a

# Run only tests affected by your changes (default)
veri

# Watch mode with instant feedback
veri -w

# Run with coverage
veri --cov

# Parallel execution
veri --workers auto
```

## 🎯 Why veri?

**Traditional pytest workflow:**
```bash
# Slow: re-collects tests in each worker, runs everything
pytest -n auto  # 45 seconds

# Or: limited change detection
pytest --lf     # only last failed, misses related tests
```

**With veri:**
```bash
# Fast: single collection, smart selection, parallel execution  
veri             # 8 seconds (only affected tests)
veri -w          # <300ms feedback on changes
```

## 🏗️ How It Works

veri builds a static analysis graph of your codebase to understand dependencies:

1. **📊 Static Analysis**: Parses import relationships via AST
2. **🧠 Smart Selection**: Runs only tests affected by changed files
3. **⚡ Fast Execution**: Single collection phase shared across workers
4. **🔄 Incremental Updates**: Maintains cache for instant re-runs

### Example Impact Analysis

```bash
# Edit src/parser.py
veri --explain

# Output:
# Changed files (1):
#   src/parser.py
# 
# Impacted tests (3):
#   tests/test_parser.py::test_parse_basic  (direct import)
#   tests/test_ast.py::test_ast_parsing     (via src.ast -> src.parser)
#   tests/integration/test_pipeline.py     (via src.pipeline -> src.parser)
```

## 🔧 Configuration

veri works with zero configuration but can be customized:

### pyproject.toml
```toml
[tool.veri]
workers = "auto"
coverage = true
watch_ignore = ["*.log", "tmp/"]

[tool.veri.ci]
shards = 4
junit_xml = "reports/junit.xml"
```

### veri.toml (alternative)
```toml
workers = 8
coverage = true

[ci]
shards = 4
```

## 🚀 CI Integration

### GitHub Actions
```yaml
- name: Install veri
  run: uv tool install veri

- name: Run tests
  run: veri --cov --junit-xml reports/junit.xml
```

### Multi-shard CI
```yaml
strategy:
  matrix:
    shard: [0, 1, 2, 3]
steps:
  - run: veri split --ci 4 > shards.json  # Only on shard 0
  - run: veri shard --ci ${{ matrix.shard }} --junit-xml reports/junit-${{ matrix.shard }}.xml
```

## 📊 Performance

Typical improvements over `pytest -n auto`:

- **Collection**: 2-5x faster (single pass vs per-worker)
- **Changed code runs**: 10-50x faster (impact analysis vs full run)  
- **Watch mode**: <300ms vs 5-30s feedback
- **CI wall time**: 20-40% reduction with smart sharding

## 🔄 Migration from pytest

veri is designed as a drop-in replacement:

```bash
# Replace this:
pytest -n auto --cov --maxfail=1 -k "not slow"

# With this:
veri --workers auto --cov --maxfail=1 -k "not slow"
```

**Plugin compatibility:**
- ✅ Core pytest features (fixtures, parametrize, markers)
- ✅ Most testing frameworks (FastAPI, Django, async)
- ⚠️ Some plugins may need `--engine pytest` fallback

See [MIGRATION.md](docs/MIGRATION.md) for detailed migration guide.

## 🛡️ Security & Privacy

- **Plugin allowlist**: Only vetted plugins run by default
- **No telemetry**: Zero data collection by default
- **Transparent**: Open source with clear security model

See [SECURITY.md](SECURITY.md) and [TELEMETRY.md](TELEMETRY.md) for details.

## 📚 Documentation

### Getting Started
- [🚀 **Quick Start Guide**](docs/QUICKSTART.md) - Get running in 5 minutes
- [🔄 **Migration from pytest**](docs/MIGRATION.md) - Step-by-step migration guide
- [⚙️ **Configuration Reference**](docs/SPEC.md) - Complete CLI and config documentation

### Technical Documentation  
- [🏗️ **Architecture & Design**](docs/RFC.md) - Technical rationale and design decisions
- [📊 **Benchmarking Plan**](docs/BENCHPLAN.md) - Performance methodology and targets
- [🗺️ **Roadmap**](docs/ROADMAP.md) - Product development timeline
- [🔧 **Error Reference**](docs/ERROR-COPY.md) - Troubleshooting and error messages

### Project Information
- [🤝 **Contributing**](CONTRIBUTING.md) - Development environment and guidelines
- [🛡️ **Security**](SECURITY.md) - Security model and best practices  
- [📊 **Telemetry**](TELEMETRY.md) - Privacy policy and data practices
- [🎨 **Branding**](docs/BRANDING.md) - Visual identity guidelines

## 🤝 Contributing

We welcome contributions! See [CONTRIBUTING.md](CONTRIBUTING.md) for:

- Development environment setup
- Code style guidelines  
- Testing procedures
- Pull request process

## 📄 License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.

## 🙏 Acknowledgments

Built with inspiration from:
- **pytest**: The foundation of Python testing
- **testmon**: Impact-aware testing concepts
- **pytest-xdist**: Parallel test execution
- **uv**: Modern Python tooling approach