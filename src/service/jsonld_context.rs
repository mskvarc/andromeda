use crate::{
    core::error::AndromedaError,
    jsonld::{
        JsonLdEngine,
        core_context::{NGSI_LD_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_V19, is_ngsi_ld_core_context_url},
    },
    repository::jsonld_context::{
        mapper::row_to_metadata,
        params::{DetailLevel, ListContextsParams},
        repo::{FetchedContext, JsonLdContextRepository, ListedContexts},
    },
};
use andromeda_domain::jsonld_context::{ContextGetResult, ContextKind, ContextListResult, ContextMetadata};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use chrono::Utc;
use serde_json::Value;
use sqlx::postgres::PgPool;
use tracing::debug;
use url::Url;
use urn_rs::Urn;

/// Service for managing hosted and cached JSON-LD contexts.
pub struct JsonLdContextService {
    repo: JsonLdContextRepository,
    engine: JsonLdEngine,
}

impl JsonLdContextService {
    /// Create a new JSON-LD context service backed by the given connection pool and engine.
    pub fn new(pool: &PgPool, engine: JsonLdEngine) -> JsonLdContextService {
        let repo = JsonLdContextRepository::new(pool);
        debug!("JSON-LD Context repository created!");
        JsonLdContextService { repo, engine }
    }

    /// Create a new hosted JSON-LD context from the given payload.
    pub async fn create(&self, payload: Value, base_url: Option<&str>) -> Result<Urn, AndromedaError> {
        if payload.get("@context").is_none() {
            return Err(AndromedaError::malformed_request("Payload must contain a top-level @context field"));
        }

        self.repo.create_hosted(payload, base_url).await
    }

    /// List available JSON-LD contexts, optionally filtered by kind and with detailed metadata.
    pub async fn list(&self, kind: Option<ContextKind>, detail_level: DetailLevel) -> Result<Value, AndromedaError> {
        let include_core = kind.is_none() || kind == Some(ContextKind::ImplicitlyCreated);
        let kind_ref = kind.as_ref();
        let params = ListContextsParams {
            kind: kind_ref,
            detail_level,
            after: None,
            limit: LIST_DEFAULT_LIMIT,
        };
        let listed = self.repo.list(params).await?;

        let dto = match listed {
            ListedContexts::Detailed(rows) => {
                let metas: Result<Vec<ContextMetadata>, AndromedaError> = rows
                    .into_iter()
                    .map(|r| row_to_metadata(r).map_err(|e| AndromedaError::internal(e.to_string())))
                    .collect();
                ContextListResult::Detailed(metas?)
            }
            ListedContexts::Summary(rows) => ContextListResult::Urls(rows.into_iter().map(|r| r.url.unwrap_or(r.id)).collect()),
        };

        match dto {
            ContextListResult::Detailed(mut metas) => {
                if include_core {
                    metas.push(build_core_context_metadata(NGSI_LD_CORE_CONTEXT_URL)?);
                }
                Ok(serde_json::to_value(metas)?)
            }
            ContextListResult::Urls(mut urls) => {
                if include_core {
                    urls.push(NGSI_LD_CORE_CONTEXT_URL.to_string());
                }
                Ok(serde_json::to_value(urls)?)
            }
        }
    }

    /// Retrieve a JSON-LD context by its identifier, returning either the payload or metadata.
    pub async fn get(&self, identifier: &str, detail_level: DetailLevel) -> Result<Value, AndromedaError> {
        if is_core_context_url(identifier) {
            return match detail_level {
                DetailLevel::Full => Ok(serde_json::to_value(build_core_context_metadata(identifier)?)?),
                DetailLevel::Summary => core_context_payload(),
            };
        }

        let fetched = self
            .repo
            .get(identifier, detail_level)
            .await?
            .ok_or_else(|| AndromedaError::context_not_found(identifier.to_string()))?;

        if detail_level == DetailLevel::Summary {
            let _ = self.repo.record_access(identifier).await;
        }

        let result = match fetched {
            FetchedContext::Detailed(row) => {
                let meta = row_to_metadata(*row).map_err(|e| AndromedaError::internal(e.to_string()))?;
                ContextGetResult::Detailed(Box::new(meta))
            }
            FetchedContext::Payload(value) => ContextGetResult::Payload(value),
        };

        match result {
            ContextGetResult::Detailed(meta) => Ok(serde_json::to_value(meta)?),
            ContextGetResult::Payload(value) => Ok(value),
        }
    }

    /// Delete a JSON-LD context, or reload it from its source URL when `reload` is true.
    pub async fn delete(&self, identifier: &str, reload: bool) -> Result<(), AndromedaError> {
        if reload {
            let row = self
                .repo
                .get_kind_and_url(identifier)
                .await?
                .ok_or_else(|| AndromedaError::jsonld_processing(format!("Context not found: {}", identifier)))?;

            let (id, kind, url) = row;

            if kind != ContextKind::Cached {
                return Err(AndromedaError::jsonld_processing("Cannot reload a non-cached context"));
            }

            let source_url = url.ok_or_else(|| AndromedaError::jsonld_processing("Cached context has no source URL"))?;

            let resolved = self
                .engine
                .fetch_remote_context(&source_url)
                .await
                .map_err(|e| AndromedaError::context_not_available(format!("Failed to re-download context from {}: {}", source_url, e)))?;

            let new_payload = serde_json::json!({ "@context": resolved });

            self.repo.replace_payload(&id, new_payload).await?;

            return Ok(());
        }

        if self.repo.delete(identifier).await? == 0 {
            return Err(AndromedaError::context_not_found(identifier.to_string()));
        }
        Ok(())
    }
}

/// Default page size for `list()` keyset pagination.
const LIST_DEFAULT_LIMIT: u32 = 1000;

fn is_core_context_url(identifier: &str) -> bool {
    is_ngsi_ld_core_context_url(identifier)
}

fn core_context_payload() -> Result<Value, AndromedaError> {
    serde_json::from_str(NGSI_LD_CORE_CONTEXT_V19).map_err(|e| AndromedaError::internal(format!("Embedded core context JSON is invalid: {}", e)))
}

fn build_core_context_metadata(identifier: &str) -> Result<ContextMetadata, AndromedaError> {
    let now = Utc::now();
    let url: Url = identifier
        .parse()
        .map_err(|e| AndromedaError::internal(format!("Core context URL is invalid: {}", e)))?;
    let local_id = Uri::Url(url.clone()); // clone: need url for both fields
    Ok(ContextMetadata::builder()
        .url(Some(url))
        .local_id(local_id)
        .kind(ContextKind::ImplicitlyCreated)
        .created_at(now)
        .expires_at(None)
        .last_usage(now)
        .number_of_hits(0)
        .extra_info(None)
        .build())
}

#[cfg(test)]
mod tests {
    use super::{build_core_context_metadata, core_context_payload, is_core_context_url};
    use andromeda_domain::jsonld_context::ContextKind;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use url::Url;

    #[test]
    fn core_context_url_is_recognized() {
        assert!(is_core_context_url("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld"));
        assert!(is_core_context_url("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"));
        assert!(!is_core_context_url("https://example.com/context.jsonld"));
    }

    #[test]
    fn core_context_details_include_kind_and_url() {
        let identifier = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld";
        let metadata = build_core_context_metadata(identifier).expect("valid core context metadata");
        pretty_assertions::assert_eq!(metadata.kind, ContextKind::ImplicitlyCreated);
        let expected_url: Url = identifier.parse().expect("valid URL");
        pretty_assertions::assert_eq!(metadata.url, Some(expected_url.clone()));
        pretty_assertions::assert_eq!(metadata.local_id, Uri::Url(expected_url));
    }

    #[test]
    fn core_context_payload_is_valid_json() {
        let payload = core_context_payload().expect("core context payload");
        assert!(payload.get("@context").is_some());
    }
}
