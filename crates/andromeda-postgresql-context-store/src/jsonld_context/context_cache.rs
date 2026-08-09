//! Direct PostgreSQL implementation of the JSON-LD resolver's technical cache port.

use crate::jsonld_context::repo::JsonLdContextRepository;
use andromeda_jsonld::context_cache::{ContextCache, ContextCacheError};
use andromeda_ngsi_ld_lexicon::uri::NGSI_LD_URN_NID;
use async_trait::async_trait;
use iri_rs::IriBuf;
use serde_json::Value;
use sqlx::types::Json;
use urn_rs::UrnBuilder;
use uuid::Uuid;

#[async_trait]
impl ContextCache for JsonLdContextRepository {
    async fn get(&self, url: &IriBuf) -> Result<Option<Value>, ContextCacheError> {
        let row: Option<(Json<Value>,)> = sqlx::query_as("SELECT payload FROM jsonld_contexts WHERE url = $1 OR id = $1")
            .bind(url.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| ContextCacheError::Read(error.to_string()))?;
        Ok(row.map(|value| value.0.0))
    }

    async fn put(&self, url: &IriBuf, payload: &Value) -> Result<(), ContextCacheError> {
        let local_id = cached_context_identifier()?;
        sqlx::query(
            "INSERT INTO jsonld_contexts (id, url, kind, payload, created_at, last_usage, number_of_hits) VALUES ($1, $2, 'Cached', $3, NOW(), NOW(), 0) ON CONFLICT (url) DO NOTHING",
        )
        .bind(local_id)
        .bind(url.as_str())
        .bind(payload)
        .execute(&self.pool)
        .await
        .map_err(|error| ContextCacheError::Write(error.to_string()))?;
        Ok(())
    }
}

/// Mint the local identifier used for a newly cached remote Context.
fn cached_context_identifier() -> Result<String, ContextCacheError> {
    UrnBuilder::new(NGSI_LD_URN_NID, &format!("Context:{}", Uuid::new_v4()))
        .build()
        .map(|identifier| identifier.to_string())
        .map_err(|error| ContextCacheError::Write(error.to_string()))
}

#[cfg(test)]
mod tests {
    use crate::jsonld_context::context_cache::cached_context_identifier;
    use andromeda_jsonld::context_cache::ContextCacheError;

    #[test]
    fn cached_context_identifier_uses_the_ngsi_ld_namespace() -> Result<(), ContextCacheError> {
        assert!(cached_context_identifier()?.starts_with("urn:ngsi-ld:Context:"));
        Ok(())
    }
}
