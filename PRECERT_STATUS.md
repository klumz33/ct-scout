# Precertificate Parsing Implementation Status

**Date:** 2025-12-13
**Issue:** Low throughput and parsing warnings

## Current Situation

### Observed Behavior
- **Throughput:** ~1,300 msg/min (lower than expected)
- **Parsing errors:** Many warnings with `UnexpectedTag { expected: Some(Tag(16)), actual: Tag(0) }`
- **Error pattern:** Affects entries across multiple CT logs

### Sample Error
```
WARN ct_scout::ct_log::monitor: https://ct.cloudflare.com/logs/nimbus2027/:
Failed to parse certificate at index 31526: Failed to parse certificate from DER:
Error(Der(UnexpectedTag { expected: Some(Tag(16)), actual: Tag(0) }))
```

## Implementation Attempted

Implemented precertificate parsing support in `src/cert_parser.rs`:

### Entry Type Detection (RFC 6962)
```rust
// MerkleTreeLeaf structure:
// Bytes 0-1: Version (0x00) + MerkleLeafType (0x00)
// Bytes 2-9: Timestamp (8 bytes)
// Bytes 10-11: LogEntryType (0x0000 for x509_entry, 0x0001 for precert_entry)

let entry_type = ((leaf_bytes[10] as u16) << 8) | (leaf_bytes[11] as u16);

match entry_type {
    0 => {
        // x509_entry: offset 12-14 = cert length, offset 15+ = DER certificate
    }
    1 => {
        // precert_entry: offset 12-43 = IssuerKeyHash, offset 44-46 = TBS length, offset 47+ = TBSCertificate
    }
}
```

### Problems Identified

1. **DER Parsing Errors**: `X509Certificate::from_der()` fails on extracted bytes with "UnexpectedTag"
2. **Root Cause Unknown**: Could be:
   - Incorrect byte offset calculations
   - TBSCertificate != X509Certificate (different ASN.1 structures)
   - Length field endianness issues
   - CT log format variations

## RFC 6962 Specification Review

### MerkleTreeLeaf Structure
```
struct {
    Version version;                // 1 byte
    MerkleLeafType leaf_type;       // 1 byte: 0x00
    uint64 timestamp;               // 8 bytes
    LogEntryType entry_type;        // 2 bytes: 0x0000 or 0x0001
    select (entry_type) {
        case x509_entry: ASN.1Cert;
        case precert_entry: PreCert;
    } entry;
} MerkleTreeLeaf;
```

### x509_entry Format
```
opaque ASN.1Cert<1..2^24-1>;  // 3-byte length (big-endian) + DER bytes
```

### precert_entry Format
```
struct {
    opaque issuer_key_hash[32];                 // SHA-256 hash
    TBSCertificate tbs_certificate<1..2^24-1>;  // 3-byte length + DER bytes
} PreCert;
```

## Key Questions

1. **Is TBSCertificate parseable with `X509Certificate::from_der()`?**
   - TBSCertificate is the "to be signed" part of an X.509 certificate
   - May require different parser from x509-parser crate
   - Need to check if x509-parser has `TbsCertificate::from_der()`

2. **Are we seeing entry type 1 (precerts) at all?**
   - Need debug logging to confirm entry_type values
   - All errors might be from type 0 parsing (not precerts)

3. **Are byte offsets correct?**
   - Need to verify with raw hex dump of leaf_input
   - Check endianness of length fields

## Next Steps

### Immediate Debugging
1. Add debug logging for entry_type values
2. Log first 50 bytes of leaf_input for failed entries (hex dump)
3. Verify we're actually encountering type 1 (precert) entries

### Alternative Approaches

#### Option A: Use TbsCertificate Parser
```rust
// For precert_entry (type 1):
use x509_parser::certificate::TbsCertificateParser;

let (_, tbs) = TbsCertificateParser::new().parse(&tbs_der)?;
// Extract SAN from tbs.extensions()
```

#### Option B: Parse Extra Data Field
RFC 6962 also includes an `extra_data` field in LogEntry that contains certificate chains. For precertificates, this contains the PreCertificate chain which has all domain information.

```rust
pub struct LogEntry {
    pub leaf_input: String,      // MerkleTreeLeaf
    pub extra_data: String,       // PrecertChainEntry or CertificateChain
}
```

For precert_entry, extra_data contains:
```
struct {
    ASN.1Cert pre_certificate;              // Full precertificate with poison extension
    ASN.1Cert precertificate_chain<0..2^24-1>;
} PrecertChainEntry;
```

**This might be easier!** Parse the full pre_certificate from extra_data instead of the TBSCertificate from leaf_input.

#### Option C: Skip Precertificates
Simplest short-term solution:
- Only parse type 0 (x509_entry)
- Skip type 1 (precert_entry) to eliminate errors
- Accept slightly lower coverage
- Revisit after Phase 1 is stable

## Coverage Impact

Typical CT log composition:
- ~70-80% final certificates (type 0)
- ~20-30% precertificates (type 1)

**Skipping precerts reduces coverage by ~20-30% but eliminates all parsing errors.**

## Recommendation

**Short-term (today):**
1. Add debug logging to identify what we're actually parsing
2. Implement Option B (parse extra_data field) for precerts - likely most robust
3. If Option B fails, implement Option C (skip precerts) to stabilize

**Medium-term:**
- Properly parse TBSCertificate using correct x509-parser API
- Handle all CT log variations
- Optimize for full 100% coverage

## Performance Note

Current throughput of ~1,300 msg/min might not be related to parsing errors:
- Many Google CT logs are very slow or idle
- Some logs only receive a few certs per minute
- Cloudflare and Sectigo logs are more active
- Total throughput depends on which logs have new certificates

Need to test over longer period (1-2 hours) to get accurate average throughput.

---

**Status:** Debugging in progress
**Blocker:** DER parsing errors on precertificates
**Priority:** High (affects coverage and error noise)
