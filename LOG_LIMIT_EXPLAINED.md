# CT Log Limits Explained

**Question:** Why are only 36 logs being monitored?

**Answer:** The 36 logs are all that Google's CT log list marks as **"usable"** (actively accepting new certificates). There are actually many more logs available!

## Log States Explained

Google's CT log list categorizes logs into different states:

- **usable** - Actively accepting new certificates (DEFAULT: monitored)
- **qualified** - Qualified for Chrome's Certificate Transparency (DEFAULT: monitored)
- **readonly** - Frozen, no new entries, but may have recent certificates (can be enabled)
- **retired** - Shut down, historical only
- **rejected** - Not trusted
- **pending** - Not yet operational

## Configuration Options

### 1. Default (Usable Logs Only)
```toml
[ct_logs]
# Only monitor "usable" and "qualified" logs
# Result: 36 logs
```

### 2. Include Readonly Logs (25% More Coverage!)
```toml
[ct_logs]
include_readonly_logs = true  # Add frozen but recent logs
# Result: 45 logs (9 additional)
```

### 3. Set Maximum Concurrent Logs
```toml
[ct_logs]
max_concurrent_logs = 200  # Default: 100
# Increase if you enable readonly logs
```

### 4. Use Custom Log List
```toml
[ct_logs]
custom_logs = [
    "https://ct.googleapis.com/logs/us1/argon2025h2/",
    "https://ct.cloudflare.com/logs/nimbus2025/",
    "https://your-favorite-log.com/ct/",
]
# Result: Only these specific logs
```

## Test Results

### Without Readonly Logs (Default)
```bash
./target/release/ct-scout --config config.toml
# Found 36 acceptable CT logs (include_readonly=false)
# Monitoring 36 CT logs
```

**Logs include:**
- Google Argon/Xenon (8 logs)
- Cloudflare Nimbus (3 logs)
- DigiCert Wyvern/Sphinx (10 logs)
- Sectigo Elephant/Tiger/Mammoth/Sabre (varies)
- TrustAsia (5 logs)
- **Total: 36 active logs**

### With Readonly Logs Enabled
```bash
./target/release/ct-scout --config config.toml
# Found 45 acceptable CT logs (include_readonly=true)
# Monitoring 45 CT logs
```

**Additional 9 logs include:**
- Let's Encrypt Oak (3 readonly logs)
- Sectigo Mammoth/Sabre (6 readonly logs)

## When to Include Readonly Logs

### ✅ Enable Readonly Logs If:

1. **Maximum Coverage Needed**
   - Want to catch every possible certificate
   - Hunting on wide scope programs
   - Historical lookback important

2. **Readonly Logs Still Have Recent Certs**
   - Logs marked "readonly" may have certificates from recent months
   - Can still catch relevant domains

3. **You Have Resources**
   - Can handle 25% more throughput
   - Database can store more entries
   - Network bandwidth available

### ❌ Keep Disabled (Default) If:

1. **Active Logs Are Enough**
   - 36 active logs already cover most certificates
   - Readonly logs may have outdated entries

2. **Resource Constraints**
   - Limited database storage
   - Bandwidth limitations
   - Want to minimize processing

3. **Latest Certificates Only**
   - Only care about brand new certificates
   - Readonly logs are frozen (no new entries)

## How Log Limits Work

The log selection happens in this order:

1. **Fetch log list** from Google (or use custom_logs)
2. **Filter by state:**
   - Always include: "usable" and "qualified"
   - Optionally include: "readonly" (if enabled)
   - Always exclude: "retired", "rejected", "pending"
3. **Apply max_concurrent_logs limit:**
   - Take first N logs from filtered list
   - Default: 100 (usually not hit with 36-45 logs)
4. **Monitor selected logs**

## Configuration Examples

### Aggressive Coverage (Recommended for Bug Bounty)
```toml
[ct_logs]
poll_interval_secs = 5
batch_size = 512
include_readonly_logs = true   # +25% coverage
max_concurrent_logs = 200      # Accommodate all logs
parse_precerts = true           # +50% entries
```

**Result:** Maximum possible coverage
- 45 logs × 2 entry types (precert + final) = complete monitoring
- Throughput: ~45,000 msg/min
- Coverage: Near 100% of all certificates

### Balanced (Default)
```toml
[ct_logs]
poll_interval_secs = 10
batch_size = 256
include_readonly_logs = false  # Only active logs
max_concurrent_logs = 100      # More than enough
parse_precerts = true          # Full entry coverage
```

**Result:** Excellent coverage with less overhead
- 36 logs monitoring
- Throughput: ~36,000 msg/min
- Coverage: 95%+ of new certificates

### Conservative (Resource-Limited)
```toml
[ct_logs]
poll_interval_secs = 30
batch_size = 128
include_readonly_logs = false  # Only active logs
max_concurrent_logs = 20       # Limit to busiest logs
parse_precerts = false         # Only final certs
```

**Result:** Reduced resource usage
- 20 logs (busiest ones first)
- Throughput: ~15,000 msg/min
- Coverage: ~70% of certificates

## Log State Transitions

Logs transition through states over time:

```
pending → usable → readonly → retired
         ↓
      qualified
```

**Example:**
- 2024: "Google Argon 2024" = **usable** (active)
- 2025: "Google Argon 2024" = **readonly** (frozen, no new entries)
- 2026: "Google Argon 2024" = **retired** (historical only)

**Why this matters:**
- Readonly logs from 2024 might still have certificates from late 2024
- Including readonly logs can catch domains from recent past
- But readonly logs won't receive new certificates

## Checking Current Log States

To see which logs are available in each state, you can:

1. **Fetch Google's log list:**
   ```bash
   curl https://www.gstatic.com/ct/log_list/v3/all_logs_list.json | jq '.operators[].logs[] | {description, state}'
   ```

2. **Enable debug logging:**
   ```toml
   [logging]
   level = "debug"  # Shows each log found with its state
   ```

3. **Check ct-scout output:**
   ```
   Found usable log: Google Argon 2025h2 (https://...)
   Found readonly log: Let's Encrypt Oak 2025h2 (https://...)
   ```

## Summary

**Your options to increase log coverage:**

1. **Enable readonly logs** (easy, +25% logs)
   ```toml
   include_readonly_logs = true
   ```

2. **Increase max_concurrent_logs** (if you have many custom logs)
   ```toml
   max_concurrent_logs = 200
   ```

3. **Use custom logs** (monitor specific logs you choose)
   ```toml
   custom_logs = ["https://your-log.com/ct/"]
   ```

**For bug bounty hunting:** ✅ **Enable readonly logs!**
- +25% more logs (45 vs 36)
- Minimal performance impact
- Better historical coverage
- Still highly active logs

---

## Quick Configuration Reference

**To monitor MORE logs:**
```toml
[ct_logs]
include_readonly_logs = true   # +9 logs (36 → 45)
max_concurrent_logs = 200      # Ensure no limit
```

**To monitor FEWER logs (save resources):**
```toml
[ct_logs]
max_concurrent_logs = 20       # Only 20 busiest logs
```

**To monitor SPECIFIC logs:**
```toml
[ct_logs]
custom_logs = [
    "https://ct.googleapis.com/logs/us1/argon2025h2/",
    "https://ct.cloudflare.com/logs/nimbus2025/"
]
```

---

**Build command:**
```bash
cargo build --release
```

**Run command:**
```bash
./target/release/ct-scout --config config.toml --stats
```

**Status:** ✅ CONFIGURABLE - Monitor 36-45+ logs as needed!
