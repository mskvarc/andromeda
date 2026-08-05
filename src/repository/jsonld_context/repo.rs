//! SQL-only repository for the JSON-LD context store.

use crate::{
    core::error::AndromedaError,
    repository::jsonld_context::{
        params::{DetailLevel, ListContextsParams},
        rows::{ContextMetadataRow, ContextSummaryRow},
    },
};
use andromeda_jsonld::context_cache::ContextCache;
use andromeda_ngsi_ld_lexicon::uri::NGSI_LD_URN_NID;
use andromeda_ngsi_ld_model::jsonld_context::ContextKind;
use async_trait::async_trait;
use serde_json::Value;
use sqlx::{postgres::PgPool, types::Json};
use urn_rs::{Urn, UrnBuilder};
use uuid::Uuid;

/// Summary-projection list output: either detailed rows or summary rows.
///
/// The repository returns this raw; the service layer assembles the public
/// [`andromeda_ngsi_ld_model::jsonld_context::ContextListResult`].
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
                        kind::text AS kind,
                        created_at,
                        expires_at,
                        last_usage,
                        number_of_hits,
                        extra_info
                    FROM jsonld_contexts
                    WHERE ($1::text IS NULL OR kind = $1::jsonld_context_kind)
                      AND ($2::text IS NULL OR id > $2)
                    ORDER BY id
                    LIMIT $3
                    "#,
                )
                .bind(params.kind.map(<&str>::from))
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
                    WHERE ($1::text IS NULL OR kind = $1::jsonld_context_kind)
                      AND ($2::text IS NULL OR id > $2)
                    ORDER BY id
                    LIMIT $3
                    "#,
                )
                .bind(params.kind.map(<&str>::from))
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
                        kind::text AS kind,
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
        let row_opt: Option<(String, String, Option<String>)> = sqlx::query_as(
            r#"
                SELECT id, kind::text AS kind, url
                FROM jsonld_contexts
                WHERE id = $1 OR url = $1
                "#,
        )
        .bind(identifier)
        .fetch_optional(&self.pool)
        .await?;

        row_opt
            .map(|(id, kind_label, url)| {
                let kind = kind_label
                    .parse::<ContextKind>()
                    .map_err(|e| AndromedaError::internal(format!("Invalid JSON-LD context kind '{kind_label}': {e}")))?;
                Ok((id, kind, url))
            })
            .transpose()
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

impl JsonLdContextRepository {
    /// Apply a batch of pending hit-count deltas in a single bulk UPDATE.
    ///
    /// Called by the broker's periodic flush task. URLs not yet persisted
    /// in `jsonld_contexts` are silently dropped: registry-resident
    /// contexts (core, preloaded) never get a row, so a counter bump for
    /// them is a no-op here and that is the intended behaviour.
    pub async fn flush_hits(&self, deltas: &[(String, u64)]) -> Result<(), AndromedaError> {
        if deltas.is_empty() {
            return Ok(());
        }
        let urls: Vec<&str> = deltas.iter().map(|(u, _)| u.as_str()).collect();
        let bigints: Vec<i64> = deltas.iter().map(|(_, n)| i64::try_from(*n).unwrap_or(i64::MAX)).collect();

        sqlx::query(
            r#"
            UPDATE jsonld_contexts
            SET number_of_hits = jsonld_contexts.number_of_hits + d.delta,
                last_usage = NOW()
            FROM UNNEST($1::text[], $2::bigint[]) AS d(url, delta)
            WHERE jsonld_contexts.url = d.url
            "#,
        )
        .bind(&urls)
        .bind(&bigints)
        .execute(&self.pool)
        .await?;
        Ok(())
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
        let local_id = match UrnBuilder::new(NGSI_LD_URN_NID, &format!("Context:{}", Uuid::new_v4())).build() {
            Ok(urn) => urn.to_string(),
            Err(e) => {
                tracing::trace!("Failed to mint local id for cached context {}: {}", url, e);
                return;
            }
        };
        let result = sqlx::query(
            "INSERT INTO jsonld_contexts (id, url, kind, payload, created_at, last_usage, number_of_hits) \
             VALUES ($1, $2, 'Cached', $3, NOW(), NOW(), 0) \
             ON CONFLICT (url) DO NOTHING",
        )
        .bind(local_id)
        .bind(url)
        .bind(payload)
        .execute(&self.pool)
        .await;
        if let Err(e) = result {
            tracing::trace!("Failed to persist context {} to DB cache: {}", url, e);
        }
    }
}
