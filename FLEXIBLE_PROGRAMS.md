# Flexible Program Configurations - FIXED ✅

**Date:** 2025-12-13
**Issue:** Programs required `domains` field, couldn't define programs with only hosts/IPs/CIDRs
**Status:** COMPLETE - Full flexibility implemented

## Problem

Previously, the config parser required all programs to have a `domains` field:

```toml
[[programs]]
name = "MyProgram"
domains = []  # Required, even if empty!
cidrs = ["10.0.0.0/8"]
```

This caused errors when trying to define programs with only specific hosts or IPs:

```
Error: TOML parse error at line 65, column 1
   |
65 | [[programs]]
   | ^^^^^^^^^^^^
missing field `domains`
```

## Solution Implemented

All program fields are now **optional** with `#[serde(default)]`:

- `domains` - Optional domain patterns (wildcards/suffixes)
- `hosts` - Optional exact hostnames
- `ips` - Optional specific IP addresses
- `cidrs` - Optional IP ranges

**You can now use ANY combination of these fields!**

## Valid Program Configurations

### Program with ONLY domains
```toml
[[programs]]
name = "DomainOnly"
domains = ["*.example.com", ".test.org"]
```

### Program with ONLY hosts
```toml
[[programs]]
name = "HostsOnly"
hosts = ["api.example.com", "www.example.com"]
```

### Program with ONLY IPs
```toml
[[programs]]
name = "IPsOnly"
ips = ["1.2.3.4", "5.6.7.8"]
```

### Program with ONLY CIDRs
```toml
[[programs]]
name = "CIDRsOnly"
cidrs = ["10.0.0.0/8", "192.168.1.0/24"]
```

### Program with domains AND hosts
```toml
[[programs]]
name = "DomainsAndHosts"
domains = ["*.ibm.com"]
hosts = ["ibm.com", "www.ibm.com"]
```

### Program with IPs AND CIDRs
```toml
[[programs]]
name = "IPsAndCIDRs"
ips = ["8.8.8.8"]
cidrs = ["8.8.4.0/24"]
```

### Program with ALL options
```toml
[[programs]]
name = "Everything"
domains = ["*.amazon.com"]
hosts = ["amazon.com", "aws.amazon.com"]
ips = ["54.239.28.85"]
cidrs = ["52.94.0.0/16"]
```

### Program with mixed options (domains + IPs)
```toml
[[programs]]
name = "MixedDomainsIPs"
domains = ["*.microsoft.com"]
ips = ["20.20.20.20"]
```

## How It Works

### Matching Logic

The watchlist now checks **both global and program-specific** scope:

#### For Domains/Hosts:
1. Check global exact hosts
2. Check global domain patterns
3. **Check program hosts** (NEW!)
4. **Check program domain patterns** (NEW!)

#### For IPs:
1. Check global exact IPs
2. Check global CIDR ranges
3. **Check program IPs** (NEW!)
4. **Check program CIDR ranges** (NEW!)

### Program Attribution

When a match is found, the program is correctly identified:

```
[2025-13-01 22:47:31] [+] securityscanningservices-cdx.microsoft.com
    Program: MixedDomainsIPs
```

This allows you to:
- Track which bug bounty program a domain belongs to
- Filter matches by program
- Send program-specific webhook notifications
- Query database by program name

## Files Modified

### src/config.rs
Added optional fields to `ProgramConfig`:
```rust
#[derive(Debug, Deserialize)]
pub struct ProgramConfig {
    pub name: String,
    #[serde(default)]  // NEW - makes optional
    pub domains: Vec<String>,
    #[serde(default)]  // NEW - makes optional
    pub hosts: Vec<String>,
    #[serde(default)]  // NEW - makes optional
    pub ips: Vec<String>,
    #[serde(default)]  // NEW - makes optional
    pub cidrs: Vec<String>,
}
```

### src/watchlist.rs

#### Updated Program struct:
```rust
pub struct Program {
    pub name: String,
    pub domains: Vec<String>,
    pub hosts: Vec<String>,   // NEW
    pub ips: Vec<IpAddr>,     // NEW
    pub cidrs: Vec<IpNet>,
}
```

#### Updated matching logic:
- `matches_domain()` - Now checks program hosts and domains
- `matches_ip()` - Now checks program IPs and CIDRs
- `program_for_domain()` - Now checks program hosts first, then domains
- `program_for_ip()` - Now checks program IPs first, then CIDRs

## Use Cases

### Bug Bounty Platform Integration
Define programs matching your bug bounty platforms:

```toml
[[programs]]
name = "HackerOne-IBM"
domains = ["*.ibm.com"]
hosts = ["ibm.com"]

[[programs]]
name = "Bugcrowd-Stripe"
domains = ["*.stripe.com"]
hosts = ["stripe.com"]
```

### IP-Based Programs
For programs that only provide IP ranges:

```toml
[[programs]]
name = "CloudProvider"
cidrs = ["10.0.0.0/8", "172.16.0.0/12"]
```

### Mixed Scope Programs
For programs with diverse scope:

```toml
[[programs]]
name = "Enterprise"
domains = ["*.company.com"]
hosts = ["company.com", "api.company.io"]
ips = ["1.2.3.4"]
cidrs = ["10.20.0.0/16"]
```

## Testing

Tested with configuration containing all combinations:

```bash
./target/release/ct-scout --config /tmp/flexible-programs-test.toml --stats
```

**Result:** ✅ All configurations loaded successfully, matches found and attributed correctly

## Benefits

1. **Full flexibility** - Use any combination of scope types
2. **No empty fields** - Don't need to specify unused fields
3. **Cleaner configs** - Only include what you need
4. **Program attribution** - Matches tagged with program name
5. **Better organization** - Group scope by bug bounty program

## Migration Notes

**No breaking changes!** Existing configs continue to work:

```toml
# Old format (still works):
[[programs]]
name = "OldProgram"
domains = ["*.example.com"]
cidrs = []

# New format (recommended):
[[programs]]
name = "NewProgram"
domains = ["*.example.com"]
# cidrs omitted - defaults to empty
```

## Production Ready

**All program field combinations now supported:**

- ✅ Domains only
- ✅ Hosts only
- ✅ IPs only
- ✅ CIDRs only
- ✅ Any combination of the above
- ✅ Correct program attribution
- ✅ No required fields (except `name`)
- ✅ Backward compatible

---

**Build command:**
```bash
cargo build --release
```

**Test command:**
```bash
./target/release/ct-scout --config your-config.toml --stats
```

**Status:** READY FOR USE WITH FLEXIBLE PROGRAM DEFINITIONS! 🎯
