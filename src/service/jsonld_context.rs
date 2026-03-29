use crate::{
    domain::jsonld_context::{ContextKind, ContextMetadata},
    error::AndromedaError,
    jsonld::{
        JsonLdEngine,
        core_context::{NGSI_LD_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_V19, is_ngsi_ld_core_context_url},
    },
    repository::jsonld_context::{ContextGetResult, ContextListResult, JsonLdContextRepository},
};
use chrono::Utc;
use serde_json::Value;
use sqlx::postgres::PgPool;
use tracing::debug;
use urn::Urn;

pub struct JsonLdContextService {
    repo: JsonLdContextRepository,
    engine: JsonLdEngine,
}

impl JsonLdContextService {
    pub fn new(pool: &PgPool, engine: JsonLdEngine) -> Self {
        let repo = JsonLdContextRepository::new(pool);
        debug!("JSON-LD Context repository created!");
        Self { repo, engine }
    }

    pub async fn create(&self, payload: Value, base_url: Option<&str>) -> Result<Urn, AndromedaError> {
        if payload.get("@context").is_none() {
            return Err(AndromedaError::invalid_request("Payload must contain a top-level @context field"));
        }

        self.repo.create_hosted(payload, base_url).await
    }

    pub async fn list(&self, kind: Option<ContextKind>, details: bool) -> Result<Value, AndromedaError> {
        let result = self.repo.list(kind.clone(), details).await?;
        let include_core = kind.is_none() || kind == Some(ContextKind::ImplicitlyCreated);

        match result {
            ContextListResult::Detailed(mut metas) => {
                if include_core {
                    metas.push(build_core_context_metadata(NGSI_LD_CORE_CONTEXT_URL));
                }
                Ok(serde_json::to_value(metas).unwrap())
            }
            ContextListResult::Urls(mut urls) => {
                if include_core {
                    urls.push(NGSI_LD_CORE_CONTEXT_URL.to_string());
                }
                Ok(serde_json::to_value(urls).unwrap())
            }
        }
    }

    pub async fn get(&self, identifier: &str, details: bool) -> Result<Value, AndromedaError> {
        if is_core_context_url(identifier) {
            return if details {
                Ok(serde_json::to_value(build_core_context_metadata(identifier)).unwrap())
            } else {
                core_context_payload()
            };
        }

        let result = self
            .repo
            .get(identifier, details)
            .await?
            .ok_or_else(|| AndromedaError::not_found(identifier.to_string()))?;

        // Record the access hit when serving the context payload (not for details queries)
        if !details {
            let _ = self.repo.record_access(identifier).await;
        }

        match result {
            ContextGetResult::Detailed(meta) => Ok(serde_json::to_value(meta).unwrap()),
            ContextGetResult::Payload(value) => Ok(value),
        }
    }

    pub async fn delete(&self, identifier: &str, reload: bool) -> Result<(), AndromedaError> {
        if reload {
            let row = self
                .repo
                .get_kind_and_url(identifier)
                .await?
                .ok_or_else(|| AndromedaError::bad_request_data(format!("Context not found: {}", identifier)))?;

            let (id, kind, url) = row;

            if kind != ContextKind::Cached {
                return Err(AndromedaError::bad_request_data("Cannot reload a non-cached context"));
            }

            let source_url = url.ok_or_else(|| AndromedaError::bad_request_data("Cached context has no source URL"))?;

            let resolved = self
                .engine
                .fetch_remote_context(&source_url)
                .await
                .map_err(|e| AndromedaError::ld_context_not_available(format!("Failed to re-download context from {}: {}", source_url, e)))?;

            let new_payload = serde_json::json!({ "@context": resolved });

            self.repo.replace_payload(&id, new_payload).await?;

            return Ok(());
        }

        if !self.repo.delete(identifier).await? {
            return Err(AndromedaError::not_found(identifier.to_string()));
        }
        Ok(())
    }
}

fn is_core_context_url(identifier: &str) -> bool {
    is_ngsi_ld_core_context_url(identifier)
}

fn core_context_payload() -> Result<Value, AndromedaError> {
    serde_json::from_str(NGSI_LD_CORE_CONTEXT_V19).map_err(|e| AndromedaError::internal(format!("Embedded core context JSON is invalid: {}", e)))
}

fn build_core_context_metadata(identifier: &str) -> ContextMetadata {
    let now = Utc::now();
    ContextMetadata {
        url: Some(identifier.to_string()),
        local_id: identifier.to_string(),
        kind: ContextKind::ImplicitlyCreated,
        created_at: now,
        expires_at: None,
        last_usage: now,
        number_of_hits: 0,
        extra_info: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_context_url_is_recognized() {
        assert!(is_core_context_url("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld"));
        assert!(is_core_context_url("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"));
        assert!(!is_core_context_url("https://example.com/context.jsonld"));
    }

    #[test]
    fn core_context_details_include_kind_and_url() {
        let identifier = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld";
        let metadata = build_core_context_metadata(identifier);
        assert_eq!(metadata.kind, ContextKind::ImplicitlyCreated);
        assert_eq!(metadata.url.as_deref(), Some(identifier));
        assert_eq!(metadata.local_id, identifier);
    }

    #[test]
    fn core_context_payload_is_valid_json() {
        let payload = core_context_payload().expect("core context payload");
        assert!(payload.get("@context").is_some());
    }
}
