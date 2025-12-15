// src/database/postgres.rs
use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::Row;
use tracing::{debug, info};

use super::{DatabaseBackend, MatchQuery};
use crate::types::MatchResult;

/// PostgreSQL database backend
pub struct PostgresBackend {
    pool: PgPool,
}

impl PostgresBackend {
    /// Create new PostgreSQL backend
    pub async fn new(database_url: &str, max_connections: u32) -> Result<Self> {
        info!("Connecting to PostgreSQL database");

        // Clean connection string by removing unsupported parameters
        // sqlx 0.8.x doesn't recognize 'channel_binding' parameter from Neon
        let cleaned_url = Self::clean_connection_string(database_url);

        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(&cleaned_url)
            .await
            .context("Failed to connect to PostgreSQL database")?;

        info!("Connected to PostgreSQL successfully");

        Ok(Self { pool })
    }

    /// Remove unsupported connection string parameters
    /// Prevents warnings from sqlx about unrecognized parameters
    fn clean_connection_string(url_str: &str) -> String {
        use url::Url;

        // Try to parse as URL and remove unsupported query parameters
        if let Ok(mut url) = Url::parse(url_str) {
            // List of parameters that sqlx doesn't recognize but are safe to remove
            let unsupported_params = ["channel_binding"];

            // Filter out unsupported parameters
            let cleaned_pairs: Vec<(String, String)> = url
                .query_pairs()
                .filter(|(key, _)| !unsupported_params.contains(&key.as_ref()))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();

            // Clear and rebuild query string
            url.query_pairs_mut().clear();
            for (key, value) in cleaned_pairs {
                url.query_pairs_mut().append_pair(&key, &value);
            }

            url.to_string()
        } else {
            // If URL parsing fails, return original
            url_str.to_string()
        }
    }

    /// Run database migrations
    pub async fn migrate(&self) -> Result<()> {
        info!("Running database migrations");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS ct_log_state (
                log_url TEXT PRIMARY KEY,
                last_index BIGINT NOT NULL,
                last_updated TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create ct_log_state table")?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS matches (
                id BIGSERIAL PRIMARY KEY,
                timestamp BIGINT NOT NULL,
                matched_domain TEXT NOT NULL,
                all_domains TEXT[] NOT NULL,
                cert_index BIGINT,
                not_before BIGINT,
                not_after BIGINT,
                fingerprint TEXT,
                program_name TEXT,
                seen_unix DOUBLE PRECISION,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create matches table")?;

        // Create indices for performance
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_matches_matched_domain
            ON matches(matched_domain)
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create index on matched_domain")?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_matches_timestamp
            ON matches(timestamp DESC)
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create index on timestamp")?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_matches_program_name
            ON matches(program_name)
            WHERE program_name IS NOT NULL
            "#,
        )
        .execute(&self.pool)
        .await
        .context("Failed to create index on program_name")?;

        info!("Database migrations completed successfully");

        Ok(())
    }

    /// Close the database connection pool
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

#[async_trait]
impl DatabaseBackend for PostgresBackend {
    async fn save_match(&self, match_result: &MatchResult) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO matches (
                timestamp, matched_domain, all_domains, cert_index,
                not_before, not_after, fingerprint, program_name, seen_unix
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(match_result.timestamp as i64)
        .bind(&match_result.matched_domain)
        .bind(&match_result.all_domains)
        .bind(match_result.cert_index.map(|i| i as i64))
        .bind(match_result.not_before.map(|i| i as i64))
        .bind(match_result.not_after.map(|i| i as i64))
        .bind(&match_result.fingerprint)
        .bind(&match_result.program_name)
        .bind(match_result.seen_unix)
        .execute(&self.pool)
        .await
        .context("Failed to insert match into database")?;

        debug!("Saved match to database: {}", match_result.matched_domain);

        Ok(())
    }

    async fn get_matches(&self, query: MatchQuery) -> Result<Vec<MatchResult>> {
        let mut sql = String::from(
            r#"
            SELECT timestamp, matched_domain, all_domains, cert_index,
                   not_before, not_after, fingerprint, program_name, seen_unix
            FROM matches
            WHERE 1=1
            "#,
        );

        let mut bind_count = 0;
        let mut bindings: Vec<String> = Vec::new();

        // Build dynamic query
        if let Some(ref pattern) = query.domain_pattern {
            bind_count += 1;
            sql.push_str(&format!(" AND matched_domain LIKE ${}", bind_count));
            bindings.push(pattern.replace('*', "%"));
        }

        if let Some(since) = query.since {
            bind_count += 1;
            sql.push_str(&format!(" AND timestamp >= ${}", bind_count));
            bindings.push(since.to_string());
        }

        if let Some(until) = query.until {
            bind_count += 1;
            sql.push_str(&format!(" AND timestamp <= ${}", bind_count));
            bindings.push(until.to_string());
        }

        if let Some(ref program) = query.program_name {
            bind_count += 1;
            sql.push_str(&format!(" AND program_name = ${}", bind_count));
            bindings.push(program.clone());
        }

        sql.push_str(" ORDER BY timestamp DESC");

        if let Some(limit) = query.limit {
            bind_count += 1;
            sql.push_str(&format!(" LIMIT ${}", bind_count));
            bindings.push(limit.to_string());
        }

        if let Some(offset) = query.offset {
            bind_count += 1;
            sql.push_str(&format!(" OFFSET ${}", bind_count));
            bindings.push(offset.to_string());
        }

        // Execute query with dynamic bindings
        let mut query_builder = sqlx::query(&sql);
        for binding in &bindings {
            query_builder = query_builder.bind(binding);
        }

        let rows = query_builder
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch matches from database")?;

        let mut results = Vec::new();
        for row in rows {
            results.push(MatchResult {
                timestamp: row.get::<i64, _>("timestamp") as u64,
                matched_domain: row.get("matched_domain"),
                all_domains: row.get("all_domains"),
                cert_index: row.get::<Option<i64>, _>("cert_index").map(|i| i as u64),
                not_before: row.get::<Option<i64>, _>("not_before").map(|i| i as u64),
                not_after: row.get::<Option<i64>, _>("not_after").map(|i| i as u64),
                fingerprint: row.get("fingerprint"),
                program_name: row.get("program_name"),
                seen_unix: row.get("seen_unix"),
            });
        }

        debug!("Fetched {} matches from database", results.len());

        Ok(results)
    }

    async fn update_log_state(&self, log_url: &str, index: u64) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO ct_log_state (log_url, last_index, last_updated)
            VALUES ($1, $2, NOW())
            ON CONFLICT (log_url)
            DO UPDATE SET last_index = $2, last_updated = NOW()
            "#,
        )
        .bind(log_url)
        .bind(index as i64)
        .execute(&self.pool)
        .await
        .context("Failed to update CT log state")?;

        Ok(())
    }

    async fn get_log_state(&self, log_url: &str) -> Result<Option<u64>> {
        let row = sqlx::query(
            r#"
            SELECT last_index FROM ct_log_state WHERE log_url = $1
            "#,
        )
        .bind(log_url)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch CT log state")?;

        Ok(row.map(|r| r.get::<i64, _>("last_index") as u64))
    }

    async fn get_all_log_states(&self) -> Result<Vec<(String, u64)>> {
        let rows = sqlx::query(
            r#"
            SELECT log_url, last_index FROM ct_log_state
            ORDER BY log_url
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch all CT log states")?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let log_url: String = row.get("log_url");
                let last_index: i64 = row.get("last_index");
                (log_url, last_index as u64)
            })
            .collect())
    }

    async fn ping(&self) -> Result<()> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .context("Database ping failed")?;

        Ok(())
    }
}
