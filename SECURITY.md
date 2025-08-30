# Security Policy and Guidelines

**veri** takes security seriously and implements multiple layers of protection to ensure safe test execution in various environments.

## Security Features

### 1. Plugin Allowlist (Default: Enabled)

By default, veri enforces a strict allowlist of approved pytest plugins to prevent execution of potentially malicious or unsafe code.

#### Default Allowed Plugins

The following plugins are included in the default allowlist:

**Core Testing Plugins:**
- `pytest` - Core pytest framework
- `pytest-cov` - Coverage measurement
- `pytest-mock` - Mock/patch utilities
- `pytest-xdist` - Distributed testing
- `pytest-asyncio` - Async test support
- `pytest-html` - HTML reporting
- `pytest-json-report` - JSON reporting
- `pytest-benchmark` - Performance benchmarking
- `pytest-timeout` - Test timeouts
- `pytest-randomly` - Random test ordering
- `pytest-rerunfailures` - Flaky test handling
- `pytest-sugar` - Enhanced output
- `pytest-clarity` - Better assertion output

**Framework-Specific Plugins:**
- `pytest-django` - Django testing
- `pytest-flask` - Flask testing
- `pytest-tornado` - Tornado testing
- `pytest-aiohttp` - aiohttp testing

**Data/Database Plugins:**
- `pytest-datadir` - Test data management
- `pytest-postgresql` - PostgreSQL testing
- `pytest-redis` - Redis testing
- `pytest-mysql` - MySQL testing

#### Configuration

**veri.toml:**
```toml
[security]
enforce_allowlist = true
allowed_plugins = [
    "pytest-custom",
    "my-company-pytest-plugin"
]
no_network = false
```

**Environment Variables:**
```bash
# Disable allowlist (not recommended)
VERI_DISABLE_ALLOWLIST=1 veri

# Block network access
VERI_NO_NETWORK=1 veri
```

**CLI Flags:**
```bash
# Disable allowlist for this run only
veri --disable-allowlist

# Block network access
veri --no-network
```

### 2. Network Isolation

When `--no-network` is enabled, veri attempts to block network access during test execution:

- Sets environment variables to disable HTTP requests in Python libraries
- Validates that no network-related plugins are active
- Provides guidance for running tests in isolated environments

**Note:** Network isolation is advisory and depends on the underlying system and Python libraries respecting the configuration.

### 3. Security Scanning

veri includes a built-in security scanner that analyzes loaded plugins and conftest.py files for potential security issues:

#### Plugin Security Analysis
- **High Risk**: Plugins with names suggesting code execution (`exec`, `eval`)
- **Medium Risk**: Debug plugins that may expose sensitive information
- **Low Risk**: Network-related plugins

#### Conftest.py Analysis
- Detection of unsafe code execution patterns
- System command usage warnings
- Dynamic import patterns

### 4. Safe Defaults

veri ships with security-first defaults:

- **Plugin allowlist enabled by default**
- **Telemetry disabled by default**
- **Network access allowed by default** (for compatibility)
- **Conservative static analysis** (broadens selection when uncertain)

## Security Best Practices

### For Development Environments

1. **Keep the plugin allowlist enabled**
   ```bash
   # Review plugins before allowing
   veri --telemetry-status  # Check current plugins
   ```

2. **Regularly audit plugins**
   ```bash
   # List all plugins
   python -c "import pkg_resources; print([d.project_name for d in pkg_resources.working_set if 'pytest' in d.project_name])"
   ```

3. **Use specific plugin versions**
   ```toml
   [security]
   allowed_plugins = [
       "pytest-cov==5.0.0",  # Pin to specific versions
   ]
   ```

### For CI/CD Environments

1. **Enable network isolation for secure environments**
   ```yaml
   # GitHub Actions example
   - name: Run tests
     run: veri --no-network --cov
     env:
       VERI_NO_NETWORK: "1"
   ```

2. **Use read-only file systems when possible**
   ```bash
   # Docker example with read-only root
   docker run --read-only -v /tmp:/tmp myapp veri
   ```

3. **Validate plugin allowlist in CI**
   ```bash
   # Fail CI if unknown plugins are detected
   veri --explain 2>&1 | grep -q "Blocked plugins" && exit 1
   ```

### For Production Environments

1. **Disable telemetry explicitly**
   ```bash
   export DO_NOT_TRACK=1
   export VERI_NO_TELEMETRY=1
   ```

2. **Use minimal plugin sets**
   ```toml
   [security]
   allowed_plugins = ["pytest", "pytest-cov"]  # Only essential plugins
   ```

3. **Run in sandboxed environments**
   - Use containers with restricted capabilities
   - Run with minimal user permissions
   - Consider using tools like `firejail` or `bubblewrap`

## Threat Model

### What veri Protects Against

1. **Malicious Plugins**
   - Plugins that could execute arbitrary code
   - Plugins that exfiltrate sensitive data
   - Plugins with known security vulnerabilities

2. **Unsafe Test Code**
   - Dynamic imports that could load malicious modules
   - Tests that make unexpected network requests
   - Tests that access unauthorized file system paths

3. **Supply Chain Attacks**
   - Compromised pytest plugins
   - Typosquatting attacks (similar package names)
   - Dependency confusion attacks

### What veri Cannot Protect Against

1. **Malicious Test Code** - If your test files themselves contain malicious code, veri cannot prevent execution
2. **Compromised Python Environment** - If the Python interpreter or core libraries are compromised
3. **Operating System Level Attacks** - veri operates at the application level
4. **Social Engineering** - Direct manipulation of allowlists or configuration

## Security Incident Response

### If You Discover a Security Issue

1. **DO NOT** create a public GitHub issue
2. **DO** email security@veri.dev with details
3. Include:
   - Description of the vulnerability
   - Steps to reproduce
   - Potential impact assessment
   - Suggested mitigation if known

### Security Updates

- Security updates are released as patch versions
- Critical security issues may result in older version advisories
- Subscribe to security notifications at https://docs.veri.dev/security

## Security Configuration Reference

### Environment Variables

| Variable | Effect | Default |
|----------|--------|---------|
| `VERI_DISABLE_ALLOWLIST` | Disables plugin allowlist | Not set |
| `VERI_NO_NETWORK` | Blocks network access | Not set |
| `VERI_NO_TELEMETRY` | Disables telemetry | Not set |
| `DO_NOT_TRACK` | Disables telemetry | Not set |
| `NO_ANALYTICS` | Disables telemetry | Not set |

### CLI Flags

| Flag | Effect |
|------|--------|
| `--disable-allowlist` | Disables plugin allowlist for this run |
| `--no-network` | Blocks network access for this run |
| `--telemetry-status` | Shows current telemetry and plugin status |

### Configuration File

```toml
[security]
# Enable/disable plugin allowlist enforcement
enforce_allowlist = true

# List of allowed plugins (in addition to defaults)
allowed_plugins = [
    "custom-plugin",
    "company-pytest-plugin==1.0.0"
]

# Block network access during test execution
no_network = false

[telemetry]
# Enable telemetry (disabled by default)
enabled = false

# Custom telemetry endpoint
endpoint = "https://telemetry.example.com/veri"

# Collection interval in seconds
collection_interval = 300
```

## Frequently Asked Questions

### Q: Why is the plugin allowlist enabled by default?

**A:** Pytest plugins have full access to the Python interpreter and can execute arbitrary code. The allowlist ensures only vetted, commonly-used plugins are loaded by default.

### Q: How do I add a custom plugin to the allowlist?

**A:** Add it to your `veri.toml`:
```toml
[security]
allowed_plugins = ["my-custom-plugin"]
```

### Q: What happens if I use `--disable-allowlist`?

**A:** veri will load all installed pytest plugins without validation. This may be necessary for some workflows but reduces security. Use with caution.

### Q: Does `--no-network` completely block network access?

**A:** No, it's advisory. It sets environment variables and provides warnings, but complete network isolation requires OS-level controls (containers, firewalls, etc.).

### Q: Is telemetry enabled by default?

**A:** No, telemetry is disabled by default. When enabled, it only collects aggregated, anonymous usage statistics.

### Q: How do I audit what plugins are currently loaded?

**A:** Use `veri --telemetry-status` to see the current plugin and telemetry status.

---

**Last Updated:** Phase 10 Implementation (August 2025)  
**Next Review:** Phase 14 (v1.0 Release)

For the latest security information, visit: https://docs.veri.dev/security