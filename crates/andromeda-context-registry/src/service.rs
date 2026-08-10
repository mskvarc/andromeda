//! Hosted and cached Context lifecycle policy.

use crate::{
    core_context,
    creation_kind::ContextCreationKind,
    detail_level::ContextDetailLevel,
    error::ContextRegistryError,
    fetched_context::FetchedContext,
    list_query::ContextListQuery,
    listed_contexts::ListedContexts,
    mutation::ContextMutation,
    removal::ContextRemoval,
    store::ContextRegistryStore,
};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::jsonld_context::ContextKind;
use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
use iri_rs::IriBuf;
use serde_json::Value;
use std::sync::Arc;
use url::Url;
use urn_rs::Urn;

/// Maximum stored rows included in the current unpaginated HTTP representation.
const LIST_DEFAULT_LIMIT: u32 = 1000;

/// Manages hosted and cached JSON-LD Contexts independently of persistence.
pub struct ContextRegistryService {
    /// The selected hosted-context persistence implementation.
    store: Arc<dyn ContextRegistryStore>,
    /// JSON-LD resolver used for cache reload and in-process invalidation.
    engine: JsonLdEngine,
}

impl ContextRegistryService {
    /// Create a Context Registry service from persistence and JSON-LD capabilities.
    pub const fn new(store: Arc<dyn ContextRegistryStore>, engine: JsonLdEngine) -> ContextRegistryService {
        ContextRegistryService { store, engine }
    }

    /// Create an explicitly hosted Context resource.
    pub async fn create(&self, payload: Value, base_url: Option<&Url>) -> Result<Urn, ContextRegistryError> {
        if payload.get("@context").is_none() {
            return Err(ContextRegistryError::MissingTopLevelContext);
        }
        self.store.create(payload, base_url, ContextCreationKind::Hosted).await
    }

    /// Persist a compound Context created while processing another resource.
    pub async fn create_implicitly(&self, payload: Value, base_url: Option<&Url>) -> Result<Urn, ContextRegistryError> {
        if payload.get("@context").is_none() {
            return Err(ContextRegistryError::MissingTopLevelContext);
        }
        self.store.create(payload, base_url, ContextCreationKind::ImplicitlyCreated).await
    }

    /// List available Contexts, optionally filtered by lifecycle kind.
    pub async fn list(&self, kind: Option<ContextKind>, detail_level: ContextDetailLevel) -> Result<Value, ContextRegistryError> {
        let include_core = kind.is_none() || kind == Some(ContextKind::ImplicitlyCreated);
        let listed = self
            .store
            .list(ContextListQuery {
                kind: kind.as_ref(),
                detail_level,
                after: None,
                limit: LIST_DEFAULT_LIMIT,
            })
            .await?;
        match listed {
            ListedContexts::Detailed(mut metadata) => {
                if include_core {
                    metadata.push(core_context::metadata(&core_context_identifier()?)?);
                }
                serialize(metadata)
            }
            ListedContexts::Summary(mut identifiers) => {
                if include_core {
                    identifiers.push(core_context_identifier()?);
                }
                serialize(identifiers)
            }
        }
    }

    /// Retrieve a Context document or its lifecycle metadata.
    pub async fn get(&self, identifier: &Uri, detail_level: ContextDetailLevel) -> Result<Value, ContextRegistryError> {
        if core_context::is_core_context(identifier) {
            return match detail_level {
                ContextDetailLevel::Full => serialize(core_context::metadata(identifier)?),
                ContextDetailLevel::Summary => core_context::payload(),
            };
        }
        let fetched = self.store.get(identifier, detail_level).await?.ok_or_else(|| ContextRegistryError::NotFound {
            identifier: identifier.clone(), // clone: the error must own the identifier after this call returns.
        })?;
        if detail_level == ContextDetailLevel::Summary {
            let access = self.store.record_access(identifier).await?;
            if access == ContextMutation::Missing {
                return Err(ContextRegistryError::NotFound {
                    identifier: identifier.clone(), // clone: the row disappeared between its read and access update.
                });
            }
        }
        match fetched {
            FetchedContext::Detailed(metadata) => serialize(*metadata),
            FetchedContext::Payload(payload) => Ok(payload),
        }
    }

    /// Delete a Context or reload a cached Context from its source.
    pub async fn remove(&self, identifier: &Uri, removal: ContextRemoval) -> Result<(), ContextRegistryError> {
        if core_context::is_core_context(identifier) {
            return match removal {
                ContextRemoval::Delete => Err(ContextRegistryError::CoreContextDeletion),
                ContextRemoval::Reload => Ok(()),
            };
        }
        match removal {
            ContextRemoval::Delete => self.delete(identifier).await,
            ContextRemoval::Reload => self.reload(identifier).await,
        }
    }

    /// Delete one stored Context and invalidate every in-process lookup key.
    async fn delete(&self, identifier: &Uri) -> Result<(), ContextRegistryError> {
        let source_url = self.store.lookup(identifier).await?.and_then(|stored| stored.source_url);
        if self.store.delete(identifier).await? == ContextMutation::Missing {
            return Err(ContextRegistryError::NotFound {
                identifier: identifier.clone(), // clone: the error must own the identifier after this call returns.
            });
        }
        if let Some(source_url) = source_url {
            self.engine.invalidate_context(&source_url).await;
        }
        let identifier_iri = IriBuf::new(identifier.to_string()).map_err(|error| ContextRegistryError::Invariant {
            detail: format!("validated Context URI is not an IRI reference: {}", error.0),
        })?;
        self.engine.invalidate_context(&identifier_iri).await;
        Ok(())
    }

    /// Re-download one cached Context and invalidate its prior in-process value.
    async fn reload(&self, identifier: &Uri) -> Result<(), ContextRegistryError> {
        let stored = self.store.lookup(identifier).await?.ok_or_else(|| ContextRegistryError::ReloadRequiresCached {
            identifier: identifier.clone(), // clone: the error must own the identifier after this call returns.
        })?;
        if stored.kind != ContextKind::Cached {
            return Err(ContextRegistryError::ReloadRequiresCached {
                identifier: identifier.clone(), // clone: the error must own the identifier after this call returns.
            });
        }
        let source_url = stored.source_url.ok_or_else(|| ContextRegistryError::MissingSourceUrl {
            identifier: identifier.clone(), // clone: the error must own the identifier after this call returns.
        })?;
        let resolved = self
            .engine
            .fetch_remote_context(&source_url)
            .await
            .map_err(|error| ContextRegistryError::ContextUnavailable {
                source_url: source_url.clone(), // clone: the source is also needed to invalidate the previous document.
                detail: error.to_string(),
            })?;
        let payload = serde_json::json!({ "@context": resolved });
        if self.store.replace_payload(&stored.identifier, payload).await? == ContextMutation::Missing {
            return Err(ContextRegistryError::NotFound { identifier: stored.identifier });
        }
        self.engine.invalidate_context(&source_url).await;
        Ok(())
    }
}

/// Parse the compile-time vocabulary constant into the HTTP resource identifier type.
fn core_context_identifier() -> Result<Uri, ContextRegistryError> {
    NGSI_LD_CORE_CONTEXT_URL.as_str().parse().map_err(|error| ContextRegistryError::Invariant {
        detail: format!("core Context URL is invalid: {error}"),
    })
}

/// Serialize a registry representation with one policy-owned error category.
fn serialize<T: serde::Serialize>(value: T) -> Result<Value, ContextRegistryError> {
    serde_json::to_value(value).map_err(|error| ContextRegistryError::Representation { detail: error.to_string() })
}

#[cfg(test)]
mod tests {
    use crate::{error::ContextRegistryError, service::core_context_identifier};

    #[test]
    fn core_context_identifier_is_a_valid_uri() -> Result<(), ContextRegistryError> {
        let identifier = core_context_identifier()?;
        assert!(identifier.as_str().starts_with("https://uri.etsi.org/ngsi-ld/"));
        Ok(())
    }
}
