//! SQL-only repository for the JSON-LD context store.

use crate::{
    core::error::AndromedaError,
    domain::{jsonld_context::ContextKind, uri::NGSI_LD_URN_NID},
    jsonld::ContextCache,
    repository::jsonld_context::{
        params::{DetailLevel, ListContextsParams},
        rows::{ContextMetadataRow, ContextSummaryRow},
    },
};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::{postgres::PgPool, types::Json};
use urn::{Urn, UrnBuilder};
use uuid::Uuid;

/// Summary-projection list output: either detailed rows or summary rows.
///
/// The repository returns this raw; the service layer assembles the public
/// [`crate::domain::jsonld_context::ContextListResult`].
#[derive(Debug)]
pub enum ListedContexts {
    /// Full metadata projection.
    Detailed(Vec<ContextMetadataRow>),
    /// Identifier-only projection.
    Summary(Vec<ContextSummaryRow>),
}

/// Single-row fetch output: either detailed row or raw payload.
#[derive(Debug)]
pub enum FetchedContext {
    /// Full metadata row.
    Detailed(Box<ContextMetadataRow>),
    /// Raw `@context` payload JSON.
    Payload(Value),
}

/// PostgreSQL-backed repository for JSON-LD context documents.
#[derive(Debug)]
pub struct JsonLdContextRepository {
    pool: PgPool,
}

impl JsonLdContextRepository {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: &PgPool) -> JsonLdContextRepository {
        JsonLdContextRepository { pool: pool.clone() } // clone: Arc ref-count bump
    }

    /// Stores a user-provided hosted JSON-LD context and returns its URN.
    pub async fn create_hosted(&self, payload: Value, base_url: Option<&str>) -> Result<Urn, AndromedaError> {
        self.insert_with_kind(payload, base_url, "Hosted").await
    }

    /// Stores an implicitly created JSON-LD context and returns its URN.
    pub async fn create_implicitly(&self, payload: Value, base_url: Option<&str>) -> Result<Urn, AndromedaError> {
        self.insert_with_kind(payload, base_url, "ImplicitlyCreated").await
    }

    async fn insert_with_kind(&self, payload: Value, base_url: Option<&str>, kind: &str) -> Result<Urn, AndromedaError> {
        let local_id = UrnBuilder::new(NGSI_LD_URN_NID, &format!("Context:{}", Uuid::new_v4())).build()?;
        let serving_url = base_url.map(|b| format!("{}/ngsi-ld/v1/jsonldContexts/{}", b.trim_end_matches('/'), local_id));

        let sql = match kind {
            "Hosted" => {
                "INSERT INTO jsonld_contexts (id, url, kind, payload, created_at, last_usage, number_of_hits) VALUES ($1, $2, 'Hosted', $3, NOW(), NOW(), 0)"
            }
            "ImplicitlyCreated" => {
                "INSERT INTO jsonld_contexts (id, url, kind, payload, created_at, last_usage, number_of_hits) VALUES ($1, $2, 'ImplicitlyCreated', $3, NOW(), NOW(), 0)"
            }
            other => return Err(AndromedaError::internal(format!("invalid insert kind: {other}"))),
        };

        sqlx::query(sql)
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

    /// Keyset-paginated list of JSON-LD contexts.
    pub async fn list(&self, params: ListContextsParams<'_>) -> Result<ListedContexts, AndromedaError> {
        let after_str: Option<String> = params.after.map(|u| u.to_string());
        let limit = i64::from(params.limit);

        match params.detail_level {
            DetailLevel::Full => {
                let rows: Vec<ContextMetadataRow> = sqlx::query_as(
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
                    WHERE ($1::jsonld_context_kind IS NULL OR kind = $1)
                      AND ($2::text IS NULL OR id > $2)
                    ORDER BY id
                    LIMIT $3
                    "#,
                )
                .bind(params.kind)
                .bind(after_str)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?;

                Ok(ListedContexts::Detailed(rows))
            }
            DetailLevel::Summary => {
                let rows: Vec<ContextSummaryRow> = sqlx::query_as(
                    r#"
                    SELECT id, url
                    FROM jsonld_contexts
                    WHERE ($1::jsonld_context_kind IS NULL OR kind = $1)
                      AND ($2::text IS NULL OR id > $2)
                    ORDER BY id
                    LIMIT $3
                    "#,
                )
                .bind(params.kind)
                .bind(after_str)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?;

                Ok(ListedContexts::Summary(rows))
            }
        }
    }

    /// Fetches a JSON-LD context by ID or URL.
    pub async fn get(&self, identifier: &str, detail_level: DetailLevel) -> Result<Option<FetchedContext>, AndromedaError> {
        match detail_level {
            DetailLevel::Full => {
                let row_opt: Option<ContextMetadataRow> = sqlx::query_as(
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

                Ok(row_opt.map(|r| FetchedContext::Detailed(Box::new(r))))
            }
            DetailLevel::Summary => {
                let row_opt: Option<(Json<Value>,)> = sqlx::query_as("SELECT payload FROM jsonld_contexts WHERE id = $1 OR url = $1")
                    .bind(identifier)
                    .fetch_optional(&self.pool)
                    .await?;

                Ok(row_opt.map(|r| FetchedContext::Payload(r.0.0)))
            }
        }
    }

    /// Record an access hit.
    pub async fn record_access(&self, identifier: &str) -> Result<u64, AndromedaError> {
        let result = sqlx::query("UPDATE jsonld_contexts SET last_usage = NOW(), number_of_hits = number_of_hits + 1 WHERE id = $1 OR url = $1")
            .bind(identifier)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    /// Deletes a JSON-LD context by ID or URL. Returns rows affected.
    pub async fn delete(&self, identifier: &str) -> Result<u64, AndromedaError> {
        let result = sqlx::query("DELETE FROM jsonld_contexts WHERE id = $1 OR url = $1")
            .bind(identifier)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }

    /// Looks up internal id, kind, and optional URL for a context.
    pub async fn get_kind_and_url(&self, identifier: &str) -> Result<Option<(String, ContextKind, Option<String>)>, AndromedaError> {
        let row_opt: Option<(String, ContextKind, Option<String>)> = sqlx::query_as(
            r#"
                SELECT id, kind, url
                FROM jsonld_contexts
                WHERE id = $1 OR url = $1
                "#,
        )
        .bind(identifier)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row_opt)
    }

    /// Replaces the JSON-LD payload for a context identified by its internal ID.
    pub async fn replace_payload(&self, identifier: &str, payload: Value) -> Result<u64, AndromedaError> {
        let result = sqlx::query(
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

        Ok(result.rows_affected())
    }
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
