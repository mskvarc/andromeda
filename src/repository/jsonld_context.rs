//! Repository for JSON-LD context documents.
//!
//! Manages hosted, cached, and implicitly-created JSON-LD contexts stored in
//! PostgreSQL, including hit tracking and the `ContextCache` trait implementation.

use crate::{
    domain::{
        jsonld_context::{ContextKind, ContextMetadata},
        uri::{NGSI_LD_URN_NID, Uri},
    },
    error::AndromedaError,
    jsonld::ContextCache,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Row, postgres::PgPool, types::Json};
use url::Url;
use urn::{Urn, UrnBuilder};
use uuid::Uuid;

/// Whether to return summary identifiers or full metadata records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailLevel {
    /// Return only identifiers or URLs.
    Summary,
    /// Return full metadata records.
    Full,
}

/// Result of listing JSON-LD contexts, either as URLs or full metadata.
#[derive(Debug)]
pub enum ContextListResult {
    /// List of context URLs or IDs.
    Urls(Vec<String>),
    /// List of context metadata records with full details.
    Detailed(Vec<ContextMetadata>),
}

/// Result of fetching a single JSON-LD context, either its payload or metadata.
#[derive(Debug)]
pub enum ContextGetResult {
    /// The raw JSON-LD context payload.
    Payload(Value),
    /// Full context metadata including usage statistics.
    Detailed(ContextMetadata),
}

/// PostgreSQL-backed repository for JSON-LD context documents.
#[derive(Debug)]
pub struct JsonLdContextRepository {
    pool: PgPool,
}

impl JsonLdContextRepository {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: &PgPool) -> JsonLdContextRepository {
        JsonLdContextRepository { pool: pool.clone() } // clone: Arc ref-count bump (PgPool is Arc-based)
    }

    /// Stores a user-provided hosted JSON-LD context and returns its URN.
    pub async fn create_hosted(&self, payload: Value, base_url: Option<&str>) -> Result<Urn, AndromedaError> {
        let local_id = UrnBuilder::new(NGSI_LD_URN_NID, &format!("Context:{}", Uuid::new_v4())).build()?;

        let serving_url = base_url.map(|b| format!("{}/ngsi-ld/v1/jsonldContexts/{}", b.trim_end_matches('/'), local_id));

        sqlx::query(
            "INSERT INTO jsonld_contexts (id, url, kind, payload, created_at, last_usage, number_of_hits) VALUES ($1, $2, 'Hosted', $3, NOW(), NOW(), 0)",
        )
        .bind(local_id.to_string())
        .bind(&serving_url)
        .bind(payload)
        .execute(&self.pool)
        .await?;

        Ok(local_id)
    }

    /// Stores an implicitly created JSON-LD context and returns its URN.
    pub async fn create_implicitly(&self, payload: Value, base_url: Option<&str>) -> Result<Urn, AndromedaError> {
        let local_id = UrnBuilder::new(NGSI_LD_URN_NID, &format!("Context:{}", Uuid::new_v4())).build()?;

        let serving_url = base_url.map(|b| format!("{}/ngsi-ld/v1/jsonldContexts/{}", b.trim_end_matches('/'), local_id));

        sqlx::query(
            "INSERT INTO jsonld_contexts (id, url, kind, payload, created_at, last_usage, number_of_hits) VALUES ($1, $2, 'ImplicitlyCreated', $3, NOW(), NOW(), 0)",
        )
        .bind(local_id.to_string())
        .bind(&serving_url)
        .bind(payload)
        .execute(&self.pool)
        .await?;

        Ok(local_id)
    }

    /// Inserts a cached context or increments its hit count if already present.
    pub async fn ensure_cached(&self, url: &str, payload: Value) -> Result<Urn, AndromedaError> {
        let local_id = UrnBuilder::new(NGSI_LD_URN_NID, &format!("Context:{}", Uuid::new_v4())).build()?;

        let row: (String,) = sqlx::query_as(
            "INSERT INTO jsonld_contexts (id, url, kind, payload, created_at, last_usage, number_of_hits) VALUES ($1, $2, 'Cached', $3, NOW(), NOW(), 0) ON CONFLICT (url) DO UPDATE SET last_usage = NOW(), number_of_hits = jsonld_contexts.number_of_hits + 1 RETURNING id",
        )
        .bind(local_id.to_string())
        .bind(url)
        .bind(payload)
        .fetch_one(&self.pool)
        .await?;

        Ok(row.0.parse::<Urn>()?)
    }

    /// Lists JSON-LD contexts, optionally filtered by kind, as URLs or detailed metadata.
    pub async fn list(&self, kind: Option<ContextKind>, detail_level: DetailLevel) -> Result<ContextListResult, AndromedaError> {
        let kind_str = kind.map(|k| k.to_string());

        match detail_level {
            DetailLevel::Full => {
                let rows = sqlx::query(
                    r#"
                    SELECT
                        id as local_id,
                        url,
                        kind,
                        created_at,
                        expires_at,
                        last_usage,
                        number_of_hits,
                        extra_info
                    FROM jsonld_contexts
                    WHERE ($1 IS NULL OR kind = $1)
                    "#,
                )
                .bind(kind_str)
                .fetch_all(&self.pool)
                .await?;

                let metas: Result<Vec<ContextMetadata>, AndromedaError> = rows.into_iter().map(|row| row_to_context_metadata(&row)).collect();
                let metas = metas?;

                Ok(ContextListResult::Detailed(metas))
            }
            DetailLevel::Summary => {
                let rows = sqlx::query(
                    r#"
                    SELECT id, url
                    FROM jsonld_contexts
                    WHERE ($1 IS NULL OR kind = $1)
                    "#,
                )
                .bind(kind_str)
                .fetch_all(&self.pool)
                .await?;

                let urls: Vec<String> = rows
                    .into_iter()
                    .map(|row| {
                        let id: String = row.get("id");
                        let url: Option<String> = row.get("url");

                        url.unwrap_or(id)
                    })
                    .collect();

                Ok(ContextListResult::Urls(urls))
            }
        }
    }

    /// Fetches a JSON-LD context by ID or URL, returning its payload or metadata.
    pub async fn get(&self, identifier: &str, detail_level: DetailLevel) -> Result<Option<ContextGetResult>, AndromedaError> {
        match detail_level {
            DetailLevel::Full => {
                let row_opt = sqlx::query(
                    r#"
                    SELECT
                        id as local_id,
                        url,
                        kind,
                        created_at,
                        expires_at,
                        last_usage,
                        number_of_hits,
                        extra_info
                    FROM jsonld_contexts
                    WHERE id = $1 OR url = $1
                    "#,
                )
                .bind(identifier)
                .fetch_optional(&self.pool)
                .await?;

                match row_opt {
                    Some(row) => Ok(Some(ContextGetResult::Detailed(row_to_context_metadata(&row)?))),
                    None => Ok(None),
                }
            }
            DetailLevel::Summary => {
                let row_opt: Option<(Json<Value>,)> = sqlx::query_as("SELECT payload FROM jsonld_contexts WHERE id = $1 OR url = $1")
                    .bind(identifier)
                    .fetch_optional(&self.pool)
                    .await?;

                Ok(row_opt.map(|r| ContextGetResult::Payload(r.0.0)))
            }
        }
    }

    /// Record an access hit (increment number_of_hits and update last_usage).
    /// Call this explicitly when the access should be counted (e.g. external API serving).
    pub async fn record_access(&self, identifier: &str) -> Result<(), AndromedaError> {
        sqlx::query("UPDATE jsonld_contexts SET last_usage = NOW(), number_of_hits = number_of_hits + 1 WHERE id = $1 OR url = $1")
            .bind(identifier)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Deletes a JSON-LD context by ID or URL. Returns `true` if a row was removed.
    pub async fn delete(&self, identifier: &str) -> Result<bool, AndromedaError> {
        let result = sqlx::query("DELETE FROM jsonld_contexts WHERE id = $1 OR url = $1")
            .bind(identifier)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Looks up the internal ID, kind, and optional URL for a context by ID or URL.
    pub async fn get_kind_and_url(&self, identifier: &str) -> Result<Option<(String, ContextKind, Option<String>)>, AndromedaError> {
        let row_opt = sqlx::query(
            r#"
                SELECT id, kind, url
                FROM jsonld_contexts
                WHERE id = $1 OR url = $1
                "#,
        )
        .bind(identifier)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row_opt {
            let id: String = row.get("id");
            let kind_str: String = row.get("kind");
            let kind: ContextKind = kind_str
                .parse()
                .map_err(|_| AndromedaError::internal(format!("Unknown context kind in database: {}", kind_str)))?;
            let url: Option<String> = row.get("url");
            Ok(Some((id, kind, url)))
        } else {
            Ok(None)
        }
    }

    /// Replaces the JSON-LD payload for a context identified by its internal ID.
    pub async fn replace_payload(&self, identifier: &str, payload: Value) -> Result<(), AndromedaError> {
        sqlx::query(
            r#"
                UPDATE jsonld_contexts
                SET payload = $2, last_usage = NOW()
                WHERE id = $1
                "#,
        )
        .bind(identifier)
        .bind(payload)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

/// Converts a database row into a [`ContextMetadata`] domain object.
fn row_to_context_metadata(row: &sqlx::postgres::PgRow) -> Result<ContextMetadata, AndromedaError> {
    let local_id_str: String = row.get("local_id");
    let url_str: Option<String> = row.get("url");
    let kind_str: String = row.get("kind");
    let hits: i64 = row.get("number_of_hits");

    let local_id: Uri = local_id_str
        .parse()
        .map_err(|_| AndromedaError::internal(format!("Invalid URI in database: {}", local_id_str)))?;

    let url = url_str
        .map(|s| s.parse::<Url>())
        .transpose()
        .map_err(|e| AndromedaError::internal(format!("Invalid URL in database: {}", e)))?;

    let kind: ContextKind = kind_str
        .parse()
        .map_err(|_| AndromedaError::internal(format!("Unknown context kind in database: {}", kind_str)))?;

    let created_at: DateTime<Utc> = row.get("created_at");
    let expires_at: Option<DateTime<Utc>> = row.get("expires_at");
    let last_usage: DateTime<Utc> = row.get("last_usage");
    let extra_info: Option<Value> = row.get("extra_info");

    Ok(ContextMetadata {
        url,
        local_id,
        kind,
        created_at,
        expires_at,
        last_usage,
        number_of_hits: hits as u64,
        extra_info,
    })
}

#[async_trait]
impl ContextCache for JsonLdContextRepository {
    async fn get(&self, url: &str) -> Option<Value> {
        let row_opt: Option<(sqlx::types::Json<Value>,)> = sqlx::query_as("SELECT payload FROM jsonld_contexts WHERE url = $1 OR id = $1")
            .bind(url)
            .fetch_optional(&self.pool)
            .await
            .ok()?;

        row_opt.map(|r| r.0.0)
    }

    async fn put(&self, url: &str, payload: Value) {
        if let Err(e) = self.ensure_cached(url, payload).await {
            tracing::trace!("Failed to persist context {} to DB cache: {}", url, e);
        }
    }
}
