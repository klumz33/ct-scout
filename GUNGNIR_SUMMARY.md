# Gungnir CT Scanner - Source Code Analysis

**Date:** 2025-12-15
**Repository:** https://github.com/g0ldencybersec/gungnir
**Version Analyzed:** v1.3.1
**Analysis Method:** Direct source code examination

---

## Summary

Gungnir is a Go-based Certificate Transparency log scanner that **uses the exact same CT log source as ct-scout**: Google's official CT log list v3.

**Key Finding:** The "1000+ logs" marketing claim is **unsubstantiated** by the source code. Gungnir monitors ~49-60 logs from Google's official list.

---

## Architecture

### CT Log Source

**Single Source URL:**
```go
logListUrl = "https://www.gstatic.com/ct/log_list/v3/all_logs_list.json"
```

Location: `pkg/runner/runner.go`

### Log Filtering Logic

**File:** `pkg/utils/utils.go` - `PopulateLogs()` function

**Included Log States:**
- ✅ `UsableLogStatus` (29 logs)
- ✅ `QualifiedLogStatus` (7 logs)
- ✅ `PendingLogStatus` (0-5 logs, varies)
- ✅ `ReadOnlyLogStatus` (9 logs)
- ✅ `RetiredLogStatus` (4 logs)

**Excluded Log States:**
- ❌ `RejectedLogStatus` (108 logs)
- ❌ No-state logs (30 logs)

**Total Monitored:** Approximately **49-60 logs** (29+7+0-5+9+4)

### Implementation Details

**Library Used:**
```go
"github.com/google/certificate-transparency-go" v1.1.8
```

**Method:**
- Direct RFC 6962 API calls (NOT certstream)
- `GetSTH()` - Fetch Signed Tree Head
- `GetRawEntries()` - Fetch certificate entries

**Rate Limiting:**
```go
// Different per operator
Google:      1ms delay
Sectigo:     4s delay
Let's Encrypt: 1s delay
DigiCert:    1s delay
TrustAsia:   1s delay
```

**Concurrency:**
- Uses `github.com/anthdm/hollywood` actor model (v1.0.5)
- Concurrent monitoring of all logs

---

## Comparison: Gungnir vs ct-scout

| Feature | Gungnir | ct-scout v2.0 |
|---------|---------|---------------|
| **CT Log Source** | Google's list v3 | Google's list v3 |
| **Implementation** | Direct RFC 6962 | Direct RFC 6962 |
| **Library** | certificate-transparency-go | x509-parser (Rust) |
| **Log Count (Default)** | ~49-60 logs | 36 logs |
| **Usable Logs** | ✅ Yes | ✅ Yes |
| **Qualified Logs** | ✅ Yes | ✅ Yes |
| **Pending Logs** | ✅ Yes | ❌ No |
| **ReadOnly Logs** | ✅ Yes | ⚙️ Optional (flag) |
| **Retired Logs** | ✅ Yes | ⚙️ Optional (with include_all) |
| **Rejected Logs** | ❌ No | ⚙️ Optional (with include_all) |
| **No-State Logs** | ❌ No | ⚙️ Optional (with include_all) |
| **Precert Parsing** | ✅ Yes | ✅ Yes |
| **Output Formats** | stdout, JSONL | Human, JSON, CSV, Silent |
| **Webhook** | ❌ No | ✅ Yes |
| **Database** | ❌ No | ⏳ Planned |
| **Stats** | ❌ No | ✅ Yes |
| **State Persistence** | ❌ No | ✅ Yes |
| **Deduplication** | ❌ No | ✅ Yes |
| **Platform APIs** | ❌ No | ⏳ Planned |

---

## The "1000+ Logs" Mystery

### Claim Origin

The gungnir README and some articles mention "monitors 1000+ CT logs."

### Source Code Reality

**Finding:** The source code shows gungnir uses **only Google's official CT log list v3**, which contains **187 total logs**.

After filtering by state (excluding Rejected and No-State), gungnir monitors approximately **49-60 logs**.

### Possible Explanations

1. **Marketing Exaggeration** - The "1000+" might refer to certificates processed per minute, not log count
2. **Outdated Claim** - Early versions may have made ambitious claims not reflected in current code
3. **Misunderstanding** - Could refer to total historical logs ever created (not currently monitored)
4. **Entry Count** - Might refer to 1000+ entries fetched per request (batch size), not log count

### Verification

**Direct code inspection confirms:**
- Single URL: `https://www.gstatic.com/ct/log_list/v3/all_logs_list.json`
- No additional sources in `go.mod` dependencies
- No hardcoded log URLs in `utils.go`
- No environment variable overrides
- No configuration file with custom logs

**Conclusion:** Gungnir does **NOT** monitor 1000+ logs. It monitors 49-60 logs from Google's official list.

---

## How to Match Gungnir's Coverage

To achieve equivalent coverage in ct-scout:

### Option 1: Match Gungnir's Filtering (Recommended)

Add new config option:

```toml
[ct_logs]
include_gungnir_set = true  # Usable + Qualified + Pending + ReadOnly + Retired
```

**Result:** 49-60 logs (same as gungnir)

### Option 2: Exceed Gungnir's Coverage

```toml
[ct_logs]
include_all_logs = true     # All 187 logs (includes Rejected and No-State)
```

**Result:** 187 logs (3-4x more than gungnir)

### Option 3: Custom Set

```toml
[ct_logs]
include_readonly_logs = true  # Usable + Qualified + ReadOnly = 45 logs
```

**Result:** 45 logs (slightly less than gungnir, but close)

---

## ct-scout Advantages Over Gungnir

Based on source code comparison:

### 1. **More Flexible Log Selection**
- ct-scout: 3 modes (36/45/187 logs)
- gungnir: Fixed set (~49-60 logs)

### 2. **State Persistence**
- ct-scout: Resume from last index, state file
- gungnir: No state persistence (restarts from current)

### 3. **Additional Features**
- Webhook notifications
- Multiple output formats
- Deduplication
- Statistics tracking
- Progress indicators
- Root domain filtering
- Database storage (planned)
- REST API (planned)
- Platform integrations (planned)

### 4. **Better Error Handling**
- ct-scout: State saves every 100 entries, graceful shutdown
- gungnir: Minimal error recovery visible in code

### 5. **Precertificate Parsing**
- ct-scout: Configurable with parse_precerts flag
- gungnir: Always parses both (not configurable)

---

## Recommendations for ct-scout

### To Match Gungnir

1. **Add "Pending" log state** to filtering logic
2. **Create gungnir-equivalent mode**: Usable + Qualified + Pending + ReadOnly + Retired
3. No need to search for "1000+ logs" - they don't exist

### To Exceed Gungnir

Continue with current approach:
- Default: 36 logs (good for most users)
- Advanced: 187 logs (maximum coverage)
- Custom: User-specified additional_logs

### Implementation Priority

1. ✅ Log merging feature (additional_logs + Google list)
2. ✅ Log health tracking (handle 404s gracefully)
3. ✅ Add "Pending" state support
4. Optional: `include_gungnir_set` config flag for direct comparison

---

## Source Code References

### Main Files Examined

1. **cmd/gungnir/main.go**
   - Entry point, minimal logic
   - Delegates to runner package

2. **pkg/runner/runner.go**
   - Log list URL: `https://www.gstatic.com/ct/log_list/v3/all_logs_list.json`
   - Uses `GetSTH()` and `GetRawEntries()` from CT library
   - Rate limiting per operator

3. **pkg/utils/utils.go**
   - `PopulateLogs()` function
   - Filtering by log state
   - No hardcoded URLs

4. **go.mod**
   - Main dependency: `github.com/google/certificate-transparency-go v1.1.8`
   - No certstream dependencies
   - Actor model: `github.com/anthdm/hollywood v1.0.5`

---

## Conclusion

**Gungnir's actual capabilities:**
- Monitors 49-60 CT logs from Google's official list
- Direct RFC 6962 API (not certstream)
- Includes Pending and Retired logs (which ct-scout currently doesn't by default)
- No additional or secret log sources

**ct-scout's current position:**
- Can monitor up to 187 logs (3-4x more than gungnir)
- More flexible configuration
- More features (webhooks, stats, persistence, deduplication)
- Slightly different default filtering (36 vs 49-60 logs)

**To match gungnir exactly:**
- Add Pending and Retired states to default filtering
- Result: ~49-60 logs monitored

**To exceed gungnir:**
- Use `include_all_logs = true`
- Result: 187 logs monitored (already implemented)

---

**References:**
- [Gungnir GitHub Repository](https://github.com/g0ldencybersec/gungnir)
- [Google's CT Log List v3](https://www.gstatic.com/ct/log_list/v3/all_logs_list.json)
- [Google certificate-transparency-go library](https://github.com/google/certificate-transparency-go)

**Analysis Date:** 2025-12-15
