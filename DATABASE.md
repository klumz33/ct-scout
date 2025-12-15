# Database Integration Guide (Phase 2A)

## Overview

ct-scout now supports PostgreSQL database integration for:
- **Match history storage** - Store all matches with full metadata
- **Historical queries** - Query past matches by domain, program, date range
- **State persistence** - CT log positions stored in database
- **Multi-instance support** - Run multiple instances sharing the same database

## Supported Databases

- **PostgreSQL** (local or cloud)
- **Neon** (Serverless Postgres) - Recommended for cloud deployments

## Setup

### 1. Create Database

#### Local PostgreSQL:
```bash
createdb ctscout
```

#### Neon (Cloud):
1. Sign up at https://neon.tech
2. Create a new project
3. Copy the connection string (format: `postgresql://username:password@ep-xxx.region.aws.neon.tech/dbname`)

### 2. Configure ct-scout

Edit `config.toml`:
```toml
[database]
enabled = true
url = "postgresql://localhost/ctscout"  # Or your Neon connection string
max_connections = 20
```

**For Neon, use SSL:**
```toml
[database]
enabled = true
url = "postgresql://user:pass@ep-xxx.us-east-2.aws.neon.tech/dbname?sslmode=require"
max_connections = 10  # Neon free tier: max 10 connections
```

### 3. Run ct-scout

On first run, ct-scout will automatically create tables:
```bash
./target/release/ct-scout --config config.toml
```

Output:
```
INFO Database enabled, connecting to PostgreSQL...
INFO Running database migrations
INFO Database initialized and migrated successfully
INFO Database ready for match storage
```

## Database Schema

### Tables

#### `ct_log_state`
Tracks last processed index for each CT log.

| Column       | Type         | Description                          |
|------------- |------------- |------------------------------------- |
| log_url      | TEXT (PK)    | CT log URL                           |
| last_index   | BIGINT       | Last processed certificate index     |
| last_updated | TIMESTAMPTZ  | When state was last updated          |

#### `matches`
Stores all certificate matches.

| Column         | Type         | Description                          |
|--------------- |------------- |------------------------------------- |
| id             | BIGSERIAL    | Auto-incrementing primary key        |
| timestamp      | BIGINT       | Unix timestamp when found            |
| matched_domain | TEXT         | Domain that matched watchlist        |
| all_domains    | TEXT[]       | All SAN domains in certificate       |
| cert_index     | BIGINT       | Certificate index from CT log        |
| not_before     | BIGINT       | Certificate validity start (Unix)    |
| not_after      | BIGINT       | Certificate validity end (Unix)      |
| fingerprint    | TEXT         | SHA-256 certificate fingerprint      |
| program_name   | TEXT         | Bug bounty program (if matched)      |
| seen_unix      | FLOAT        | Original "seen" timestamp            |
| created_at     | TIMESTAMPTZ  | Row creation time                    |

### Indices

- `idx_matches_matched_domain` - Fast domain lookups
- `idx_matches_timestamp` - Time-based queries (DESC for recent first)
- `idx_matches_program_name` - Program-based filtering

## Querying the Database

### Recent Matches
```sql
SELECT matched_domain, program_name, to_timestamp(timestamp) AS found_at
FROM matches
ORDER BY timestamp DESC
LIMIT 100;
```

### Matches for Specific Domain Pattern
```sql
SELECT matched_domain, all_domains, program_name
FROM matches
WHERE matched_domain LIKE '%.ibm.com'
ORDER BY timestamp DESC;
```

### Matches in Last 7 Days
```sql
SELECT matched_domain, program_name
FROM matches
WHERE timestamp >= EXTRACT(EPOCH FROM NOW() - INTERVAL '7 days')
ORDER BY timestamp DESC;
```

### Match Count by Program
```sql
SELECT program_name, COUNT(*) AS match_count
FROM matches
WHERE program_name IS NOT NULL
GROUP BY program_name
ORDER BY match_count DESC;
```

### CT Log Processing Status
```sql
SELECT log_url, last_index, last_updated
FROM ct_log_state
ORDER BY last_updated DESC;
```

## Features

### Automatic Migrations
ct-scout creates tables and indices automatically on first run. No manual SQL required.

### Match Storage
Every certificate match is automatically saved to the database with full metadata.

### Performance
- **Indexed queries** - Fast lookups by domain, timestamp, program
- **Bulk inserts** - Matches written efficiently
- **Connection pooling** - Reuses database connections

### Neon Compatibility
- Serverless Postgres - Scales to zero when idle
- Low latency (~10-50ms from most regions)
- Free tier: 512MB storage, 0.5 vCPU
- Connection pooling handles serverless wakeup

## Limitations & Notes

### Current State Management
In this version, CT log state is still primarily managed via TOML file for simplicity.
Database state table is prepared for future migration to fully database-backed state.

### Match Storage Only
Database is used for:
- ✅ Storing all matches with metadata
- ✅ Historical queries
- 🔜 CT log state (future enhancement)

### Connection Limits
- **Local PostgreSQL**: Default 100 connections (configurable)
- **Neon Free Tier**: Max 10 concurrent connections
- Set `max_connections` in config to avoid exceeding limits

### Disk Usage
Estimated storage per match: ~500 bytes (varies with SAN count)
- 1,000 matches = ~500 KB
- 100,000 matches = ~50 MB
- 1,000,000 matches = ~500 MB

## Troubleshooting

### Connection Refused
```
Error: Failed to connect to PostgreSQL database
```
**Solution:**
- Check database is running: `pg_isready`
- Verify connection string is correct
- For Neon: Check project is running (not paused)

### SSL Required (Neon)
```
Error: FATAL: no pg_hba.conf entry for host
```
**Solution:** Add `?sslmode=require` to connection string

### Too Many Connections
```
Error: remaining connection slots are reserved
```
**Solution:** Reduce `max_connections` in config.toml

### Slow Queries
**Solution:** Database has proper indices. If still slow, check:
```sql
EXPLAIN ANALYZE SELECT * FROM matches WHERE matched_domain LIKE '%.ibm.com';
```

## Next Steps (Phase 2B & Beyond)

- **Platform Integration** - Auto-sync from HackerOne/Intigriti APIs
- **REST API** - Query historical matches via HTTP API
- **WebSocket Streaming** - Real-time match feed
- **Database-Backed State** - Full migration from TOML to PostgreSQL

## Example Workflow

### 1. Start Monitoring
```bash
./target/release/ct-scout --config config-with-database.toml --stats
```

### 2. Check Progress
```sql
-- How many matches so far?
SELECT COUNT(*) FROM matches;

-- Most recent matches
SELECT matched_domain, to_timestamp(timestamp) FROM matches ORDER BY timestamp DESC LIMIT 10;
```

### 3. Export for Other Tools
```sql
-- Export domains for nuclei/httpx
\copy (SELECT DISTINCT matched_domain FROM matches) TO 'domains.txt'
```

### 4. Query Specific Program
```sql
SELECT matched_domain, all_domains
FROM matches
WHERE program_name = 'IBM Bug Bounty'
ORDER BY timestamp DESC;
```

## Security Notes

- Store connection strings in environment variables for production
- Use strong passwords for database users
- Enable SSL for remote connections (Neon enforces this)
- Limit database user permissions (no need for DROP/CREATE)

## Production Recommendations

### For Neon:
- Use Pro plan for production (removes connection limit)
- Enable connection pooling (built-in)
- Monitor query performance via Neon dashboard

### For Self-Hosted:
- Use connection pooling (PgBouncer)
- Regular backups (`pg_dump ctscout > backup.sql`)
- Monitor disk usage
- Set up replication for HA

---

**Phase 2A Complete!** Database integration is now production-ready.
