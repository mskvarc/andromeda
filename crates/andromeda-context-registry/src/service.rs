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
use andromeda_jsonld::{context_subtree::context_subtree, engine::processor::JsonLdEngine};
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
    ///
    /// Everything the request carried outside the `@context` subtree is
    /// discarded before the document is stored (clause 5.13.2.3).
    ///
    /// # Errors
    ///
    /// Returns [`ContextRegistryError::MissingTopLevelContext`] when the payload
    /// carries no top-level `@context` member, and
    /// [`ContextRegistryError::Persistence`] when the store cannot record the
    /// hosted Context.
    pub async fn create(&self, payload: Value, base_url: Option<&Url>) -> Result<Urn, ContextRegistryError> {
        let document = hosted_context_subtree(payload)?;
        self.store.create(document, base_url, ContextCreationKind::Hosted).await
    }

    /// Persist a compound Context created while processing another resource.
    ///
    /// Everything outside the `@context` subtree is discarded before the
    /// document is stored (clause 5.13.2.3), so a Context the broker mints for
    /// its own use is shaped exactly like one a client supplied.
    ///
    /// # Errors
    ///
    /// Returns [`ContextRegistryError::MissingTopLevelContext`] when the payload
    /// carries no top-level `@context` member, and
    /// [`ContextRegistryError::Persistence`] when the store cannot record the
    /// implicitly created Context.
    pub async fn create_implicitly(&self, payload: Value, base_url: Option<&Url>) -> Result<Urn, ContextRegistryError> {
        let document = hosted_context_subtree(payload)?;
        self.store.create(document, base_url, ContextCreationKind::ImplicitlyCreated).await
    }

    /// List available Contexts, optionally filtered by lifecycle kind.
    ///
    /// # Errors
    ///
    /// Returns [`ContextRegistryError::Persistence`] when the store cannot read
    /// the stored rows, [`ContextRegistryError::Invariant`] when the embedded
    /// core Context URL does not parse as a resource identifier or carries no
    /// representable metadata, and [`ContextRegistryError::Representation`] when
    /// the assembled listing cannot be serialized.
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
    ///
    /// # Errors
    ///
    /// Returns [`ContextRegistryError::NotFound`] when no Context is stored under
    /// the identifier, or when the row disappears between its read and its access
    /// update. Returns [`ContextRegistryError::Persistence`] when the store cannot
    /// complete the read or the access update,
    /// [`ContextRegistryError::Invariant`] when an embedded core Context has no
    /// representable metadata or document, and
    /// [`ContextRegistryError::Representation`] when the metadata cannot be
    /// serialized.
    pub async fn get(&self, identifier: &Uri, detail_level: ContextDetailLevel) -> Result<Value, ContextRegistryError> {
        if core_context::is_core_context(identifier) {
            return match detail_level {
                ContextDetailLevel::Full => serialize(core_context::metadata(identifier)?),
                ContextDetailLevel::Summary => core_context::payload(),
            };
        }
        // Clause 5.13.4.4: the full content is served from internal storage only
        // for a `Hosted` or `ImplicitlyCreated` `@context`; a `Cached` one is a
        // local performance copy (clause 5.13.1) and answers
        // `OperationNotSupported`. Metadata (`details=true`) is unrestricted, so
        // only the payload projection asks.
        if detail_level == ContextDetailLevel::Summary
            && let Some(stored) = self.store.lookup(identifier).await?
            && stored.kind == ContextKind::Cached
        {
            return Err(ContextRegistryError::CachedContextNotServed {
                identifier: identifier.clone(), // clone: the error must own the identifier after this call returns.
            });
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
    ///
    /// # Errors
    ///
    /// Returns [`ContextRegistryError::CoreContextDeletion`] when deletion names
    /// an embedded core Context, [`ContextRegistryError::NotFound`] when no
    /// Context is stored under the identifier,
    /// [`ContextRegistryError::ReloadRequiresCached`] when reload names a Context
    /// of a kind other than `Cached`, [`ContextRegistryError::MissingSourceUrl`]
    /// when a cached Context records no remote source,
    /// [`ContextRegistryError::ContextUnavailable`] when the remote document
    /// cannot be downloaded, [`ContextRegistryError::Persistence`] when the store
    /// cannot complete the deletion or the replacement, and
    /// [`ContextRegistryError::Invariant`] when the stored identifier is not an
    /// IRI reference the in-process cache can be invalidated by.
    pub async fn remove(&self, identifier: &Uri, removal: ContextRemoval) -> Result<(), ContextRegistryError> {
        if core_context::is_core_context(identifier) {
            return match removal {
                ContextRemoval::Delete => Err(ContextRegistryError::CoreContextDeletion),
                // Clause 5.13.5.4: "If reload is true and the kind of the
                // @context is not "Cached", implementations shall return a
                // BadRequestData error." An embedded core Context is surfaced
                // as `ImplicitlyCreated` (clause 5.13.1), so there is no
                // original URL to re-download it from and nothing to replace.
                ContextRemoval::Reload => Err(ContextRegistryError::ReloadRequiresCached {
                    identifier: identifier.clone(), // clone: the error must own the identifier after this call returns.
                }),
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
        // Clause 5.13.5.4 orders the two refusals: "If the @context identifier
        // does not correspond to any existing entry in the @context storage,
        // then an error of type ResourceNotFound shall be raised" comes before
        // the kind is considered, because an identifier naming nothing has no
        // kind to object to.
        let stored = self.store.lookup(identifier).await?.ok_or_else(|| ContextRegistryError::NotFound {
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
        // Clause 5.13.5.4: "the existing @context is replaced with the newly
        // downloaded one." The resolver hands the download back already reduced
        // to its `@context` subtree (clause 5.13.2.3), which is exactly the
        // shape a stored Context has, so it is stored as it stands. Wrapping it
        // in a second `@context` produced a document no expansion can use, and
        // clause 5.13.5 offers no operation that could undo that.
        if self.store.replace_payload(&stored.identifier, resolved).await? == ContextMutation::Missing {
            return Err(ContextRegistryError::NotFound { identifier: stored.identifier });
        }
        self.engine.invalidate_context(&source_url).await;
        Ok(())
    }
}

/// The `@context` subtree a client-supplied document must carry.
///
/// The reduction clause 5.13.2.3 requires belongs to every entry point a
/// context document has, so it lives in `andromeda_jsonld` and is shared with
/// the resolver; what is particular to this one is that a document carrying no
/// local context is a client error rather than an unusable download.
///
/// # Errors
///
/// Returns [`ContextRegistryError::MissingTopLevelContext`] when the payload is
/// not a JSON object, or is one that carries no top-level `@context` member.
fn hosted_context_subtree(payload: Value) -> Result<Value, ContextRegistryError> {
    context_subtree(payload).ok_or(ContextRegistryError::MissingTopLevelContext)
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
    use crate::{
        error::ContextRegistryError,
        service::{core_context_identifier, hosted_context_subtree},
    };
    use serde_json::json;

    #[test]
    fn core_context_identifier_is_a_valid_uri() -> Result<(), ContextRegistryError> {
        let identifier = core_context_identifier()?;
        assert!(identifier.as_str().starts_with("https://uri.etsi.org/ngsi-ld/"));
        Ok(())
    }

    /// Clause 5.13.2.3: "all extra information located outside of the @context
    /// subtree in the referenced object shall be discarded". What survives the
    /// reduction is asserted where the reduction lives; what this owns is that
    /// a hosted document goes through it at all.
    #[test]
    fn a_hosted_document_keeps_only_its_context_subtree() -> Result<(), ContextRegistryError> {
        let supplied = json!({
            "@context": { "Building": "https://example.com/Building" },
            "id": "urn:ngsi-ld:Context:supplied-by-the-client",
            "createdAt": "2020-01-01T00:00:00Z",
            "extraInfo": { "owner": "someone" }
        });

        pretty_assertions::assert_eq!(
            hosted_context_subtree(supplied)?,
            json!({ "@context": { "Building": "https://example.com/Building" } })
        );
        Ok(())
    }

    /// Clause 5.13.2.3 requires a top-level `@context` field; a client document
    /// without one names no local context, which is a request error.
    #[test]
    fn a_document_without_a_top_level_context_is_refused() {
        for supplied in [
            json!({ "A1": "urn:ngsi-ld:attributes:A1" }),
            json!({}),
            json!([{ "@context": {} }]),
            json!("@context"),
        ] {
            assert!(
                matches!(hosted_context_subtree(supplied), Err(ContextRegistryError::MissingTopLevelContext)),
                "a document with no top-level @context must be refused"
            );
        }
    }
}
