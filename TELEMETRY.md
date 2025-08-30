# Telemetry and Privacy Policy

**veri** includes optional telemetry to help improve the tool. **Telemetry is disabled by default** and requires explicit opt-in.

## Privacy-First Approach

### What We Do NOT Collect

- **No source code** - We never collect, transmit, or store your code
- **No test names** - Test identifiers and names are never transmitted
- **No file paths** - Local file paths and directory structures are excluded
- **No personal data** - No usernames, emails, or personal identifiers
- **No sensitive data** - No environment variables, secrets, or configuration details
- **No test results** - We don't collect specific test outcomes or assertions

### What We Collect (When Enabled)

When telemetry is explicitly enabled, veri collects only aggregated, anonymous usage statistics:

#### Session Information
- Random session ID (generated per session, not persistent)
- veri version
- Platform (OS and architecture, e.g., "linux-x86_64")
- Python version (major.minor only, e.g., "3.12")

#### Usage Counters
- Total number of test runs
- Number of runs with coverage enabled
- Number of watch mode sessions
- Number of CI mode executions

#### Performance Metrics (Aggregated)
- Average test collection time
- Average test execution time
- Average number of tests per run
- Average number of workers used

#### Feature Usage
- Which features are used (coverage, watch mode, sharding, etc.)
- Count of feature usage (not when or how)

#### Error Categories (No Details)
- Type of errors encountered (e.g., "collection", "execution")
- Count of error types (not specific error messages)

## Example Telemetry Data

Here's an example of what gets transmitted when telemetry is enabled:

```json
{
  "session_id": "550e8400-e29b-41d4-a716-446655440000",
  "veri_version": "0.1.0",
  "platform": "linux-x86_64",
  "python_version": "3.12",
  "runs_total": 45,
  "runs_with_coverage": 12,
  "runs_watch_mode": 8,
  "runs_ci_mode": 25,
  "avg_collection_time_ms": 150,
  "avg_execution_time_ms": 2300,
  "avg_tests_per_run": 127.5,
  "avg_workers_used": 4.2,
  "features_used": {
    "coverage": 12,
    "watch": 8,
    "parallel": 30,
    "impact_analysis": 35
  },
  "error_categories": {
    "collection": 2,
    "execution": 1
  },
  "last_updated": 1692345600
}
```

## How to Enable/Disable Telemetry

### Disabled by Default
Telemetry is **disabled by default**. No data is collected unless you explicitly opt in.

### Ways to Opt Out (Multiple Options)
Even if telemetry were enabled, it can be disabled in several ways:

```bash
# Environment variables (recommended)
export DO_NOT_TRACK=1              # Universal opt-out standard
export VERI_NO_TELEMETRY=1         # veri-specific opt-out
export NO_ANALYTICS=1              # Common opt-out variable

# Configuration file
echo 'telemetry_enabled = false' >> veri.toml

# Check current status
veri --telemetry-status
```

### How to Opt In

```bash
# Environment variable
export VERI_TELEMETRY_ENABLED=1

# Configuration file
[telemetry]
enabled = true
endpoint = "https://telemetry.veri.dev"  # Optional custom endpoint
collection_interval = 300               # Optional, default 5 minutes
```

### Network Isolation Override
The `--no-network` flag completely blocks telemetry transmission regardless of settings:

```bash
veri --no-network  # Guarantees no telemetry transmission
```

## Telemetry Status

### Check Current Status
```bash
veri --telemetry-status
```

Output example:
```
📊 Telemetry Status
  Enabled: ❌ No
  Session ID: 550e8400-e29b-41d4-a716-446655440000
  Data collected:
    • Runs recorded: 0
    • Features tracked: 0
    • Error categories: 0
  Privacy guarantees:
    • No personal data: ✅
    • No code paths: ✅
    • No test names: ✅
  Opt-out methods:
    • Set VERI_NO_TELEMETRY=1
    • Set DO_NOT_TRACK=1
    • Add 'telemetry_enabled = false' to veri.toml
  More info: https://docs.veri.dev/telemetry
```

## Data Usage and Retention

### How Data Is Used
- **Improve performance** - Understand common bottlenecks
- **Feature prioritization** - See which features are most valuable
- **Error reduction** - Identify common configuration issues
- **Compatibility** - Ensure veri works across different environments

### Data Retention
- Telemetry data is retained for 12 months maximum
- Data is aggregated and anonymized before analysis
- Individual session data is not stored long-term
- No data is sold or shared with third parties

### Data Security
- All telemetry transmission uses HTTPS
- Data is encrypted at rest
- Access is restricted to core development team
- No third-party analytics services are used

## Technical Implementation

### When Data Is Sent
- Data is collected locally during veri execution
- Transmission occurs asynchronously and doesn't affect performance
- If transmission fails, data is discarded (not retried)
- No local storage of telemetry data between sessions

### Network Requests
When telemetry is enabled, veri makes HTTPS POST requests to:
- Default endpoint: `https://telemetry.veri.dev/api/v1/events`
- Custom endpoint: Configurable via `telemetry.endpoint`

### Opt-Out Verification
You can verify telemetry is disabled by:

1. **Network monitoring** - No requests to telemetry endpoints
2. **Environment check** - Set `DO_NOT_TRACK=1` and verify no transmission
3. **Code inspection** - veri is open source; verify telemetry implementation
4. **Status command** - `veri --telemetry-status` shows current state

## Compliance and Standards

### GDPR Compliance
- No personal data is collected
- No consent required (data is truly anonymous)
- Right to object: Multiple opt-out mechanisms
- Data minimization: Only essential metrics collected

### Industry Standards
- Follows [DO_NOT_TRACK](https://www.eff.org/dnt-policy) specification
- Implements privacy-by-design principles
- Uses minimal data collection approach
- Provides transparent data practices

## Frequently Asked Questions

### Q: Why does veri have telemetry at all?

**A:** Optional telemetry helps us understand how veri is used in practice, which features are valuable, and what performance improvements matter most. This leads to a better tool for everyone.

### Q: Is telemetry enabled by default?

**A:** No, telemetry is disabled by default. It requires explicit opt-in through configuration or environment variables.

### Q: Can I see what data would be sent before enabling telemetry?

**A:** Yes, use `veri --telemetry-status` to see the current data that would be transmitted.

### Q: How do I completely disable telemetry?

**A:** Set `export DO_NOT_TRACK=1` or `export VERI_NO_TELEMETRY=1`. This works even if telemetry is configured as enabled.

### Q: Does telemetry affect performance?

**A:** No, telemetry data is sent asynchronously and doesn't block test execution. If you're concerned, use `--no-network` to guarantee no network activity.

### Q: Who has access to telemetry data?

**A:** Only core veri maintainers have access to aggregated telemetry data. Raw data is never manually inspected.

### Q: Can I use a custom telemetry endpoint?

**A:** Yes, you can configure a custom endpoint in your `veri.toml`:
```toml
[telemetry]
enabled = true
endpoint = "https://my-company.com/analytics"
```

### Q: What happens if the telemetry endpoint is unavailable?

**A:** The data is discarded silently. veri never retries failed telemetry transmissions or caches data for later sending.

### Q: Is telemetry open source?

**A:** Yes, the entire telemetry implementation is open source and can be audited in the veri repository.

## Contact and Feedback

### Privacy Concerns
If you have concerns about privacy or data collection:
- Email: privacy@veri.dev
- Review the source code: https://github.com/veri-dev/veri
- File an issue: https://github.com/veri-dev/veri/issues

### Telemetry Feedback
To suggest improvements to telemetry or request additional privacy controls:
- Discussion forum: https://github.com/veri-dev/veri/discussions
- Feature requests: https://github.com/veri-dev/veri/issues

---

**Last Updated:** Phase 10 Implementation (August 2025)  
**Next Review:** Phase 14 (v1.0 Release)

For the latest privacy information, visit: https://docs.veri.dev/privacy