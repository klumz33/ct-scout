# Bug Fix: CT Log List JSON Parsing

**Date:** 2025-12-13
**Issue:** Failed to parse Google's CT log list JSON
**Status:** ✅ Fixed

## Problem

When running ct-scout, it failed with:
```
Error: Failed to parse log list JSON
Caused by:
    0: error decoding response body
    1: invalid type: map, expected unit at line 19 column 24
```

## Root Cause

Google's CT log list V3 format uses a different state structure than we expected:

**Actual JSON format:**
```json
"state": {
  "usable": {
    "timestamp": "2024-09-30T22:19:27Z"
  }
}
```

**What we had:**
```rust
pub enum LogState {
    Usable,
    Readonly,
    // ... simple enum variants
}
```

The state is not a simple string/enum, but an **object** containing nested state types with timestamps.

## Solution

Updated `src/ct_log/types.rs` to match the actual JSON structure:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateWrapper {
    #[serde(default)]
    pub usable: Option<StateTimestamp>,
    #[serde(default)]
    pub readonly: Option<StateTimestamp>,
    #[serde(default)]
    pub retired: Option<StateTimestamp>,
    #[serde(default)]
    pub rejected: Option<StateTimestamp>,
    #[serde(default)]
    pub qualified: Option<StateTimestamp>,
    #[serde(default)]
    pub pending: Option<StateTimestamp>,
}

impl StateWrapper {
    pub fn is_usable(&self) -> bool {
        self.usable.is_some() || self.qualified.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTimestamp {
    pub timestamp: String,
}
```

Also added:
- `email` field to `Operator` struct
- `#[serde(default)]` attributes to handle missing fields gracefully

## Files Modified

1. **src/ct_log/types.rs**
   - Removed simple `LogState` enum
   - Added `StateWrapper` struct with optional state types
   - Added `StateTimestamp` struct
   - Added `is_usable()` helper method

2. **src/ct_log/log_list.rs**
   - Updated to use `state.is_usable()` instead of enum matching
   - Removed `LogState` import

## Testing

After the fix:
```bash
$ ./target/release/ct-scout --config config.toml --stats

INFO Fetching CT log list from https://www.gstatic.com/ct/log_list/v3/all_logs_list.json
INFO Found 36 usable CT logs
INFO Monitoring 36 CT logs (limited by max_concurrent_logs)
INFO Starting 36 CT log monitors
```

✅ Successfully parsing Google's CT log list
✅ Identifying usable and qualified logs
✅ Monitoring multiple CT logs concurrently

## Lessons Learned

1. Always fetch and inspect the actual API response before designing data structures
2. Use `#[serde(default)]` for optional fields to make parsing more resilient
3. Google's CT log list V3 format uses tagged unions for states, not simple enums

## Related Documentation

- CT Log List V3 Schema: https://www.gstatic.com/ct/log_list/v3/all_logs_list.json
- RFC 6962 (CT Logs): https://tools.ietf.org/html/rfc6962
- Log States: usable, qualified, readonly, rejected, retired, pending
