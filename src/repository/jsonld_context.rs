use crate::{
    domain::{
        jsonld_context::{ContextKind, ContextMetadata},
        uri::NGSI_LD_URN_NID,
    },
    error::AndromedaError,
};
use async_trait::async_trait;
use json_ld_engine::ContextCache;
use serde_json::Value;
use sqlx::{postgres::PgPool, types::Json};
use urn::{Urn, UrnBuilder};
use uuid::Uuid;

pub enum ContextListResult {
    Urls(Vec<String>),
    Detailed(Vec<ContextMetadata>),
}

pub enum ContextGetResult {
    Payload(Value),
    Detailed(ContextMetadata),
}

pub struct JsonLdContextRepository {
    pool: PgPool,
}

impl JsonLdContextRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

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

    pub async fn list(&self, kind: Option<ContextKind>, details: bool) -> Result<ContextListResult, AndromedaError> {
        let kind_str = kind.map(|k| k.to_string());

        if details {
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

            let metas: Vec<ContextMetadata> = rows
                .into_iter()
                .map(|row| {
                    use sqlx::Row;
                    let local_id: String = row.get("local_id");
                    let url: Option<String> = row.get("url");
                    ContextMetadata {
                        url,
                        local_id,
                        kind: row.get("kind"),
                        created_at: row.get("created_at"),
                        expires_at: row.get("expires_at"),
                        last_usage: row.get("last_usage"),
                        number_of_hits: row.get("number_of_hits"),
                        extra_info: row.get("extra_info"),
                    }
                })
                .collect();

            Ok(ContextListResult::Detailed(metas))
        } else {
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
                    use sqlx::Row;
                    let id: String = row.get("id");
                    let url: Option<String> = row.get("url");

                    url.unwrap_or(id)
                })
                .collect();

            Ok(ContextListResult::Urls(urls))
        }
    }

    pub async fn get(&self, identifier: &str, details: bool) -> Result<Option<ContextGetResult>, AndromedaError> {
        if details {
            // details=true: SELECT only, no hit increment
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

            if let Some(row) = row_opt {
                use sqlx::Row;
                let local_id: String = row.get("local_id");
                let url: Option<String> = row.get("url");
                let meta = ContextMetadata {
                    url,
                    local_id,
                    kind: row.get("kind"),
                    created_at: row.get("created_at"),
                    expires_at: row.get("expires_at"),
                    last_usage: row.get("last_usage"),
                    number_of_hits: row.get("number_of_hits"),
                    extra_info: row.get("extra_info"),
                };
                Ok(Some(ContextGetResult::Detailed(meta)))
            } else {
                Ok(None)
            }
        } else {
            // details=false: pure read, return payload
            let row_opt: Option<(Json<Value>,)> = sqlx::query_as("SELECT payload FROM jsonld_contexts WHERE id = $1 OR url = $1")
                .bind(identifier)
                .fetch_optional(&self.pool)
                .await?;

            Ok(row_opt.map(|r| ContextGetResult::Payload(r.0.0)))
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

    pub async fn delete(&self, identifier: &str) -> Result<bool, AndromedaError> {
        let result = sqlx::query("DELETE FROM jsonld_contexts WHERE id = $1 OR url = $1")
            .bind(identifier)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

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
            use sqlx::Row;
            let id: String = row.get("id");
            let kind: ContextKind = row.get("kind");
            let url: Option<String> = row.get("url");
            Ok(Some((id, kind, url)))
        } else {
            Ok(None)
        }
    }

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
