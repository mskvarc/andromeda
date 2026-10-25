//! Hosted and cached Context lifecycle policy.

use crate::{
    context_page::ContextPage,
    creation_kind::ContextCreationKind,
    detail_level::ContextDetailLevel,
    error::ContextRegistryError,
    fetched_context::FetchedContext,
    hosted_context_locator::HostedContextLocator,
    identifier::ContextIdentifier,
    implicit_context_hosting::ImplicitContextHosting,
    insertion_policy::InsertionPolicy,
    list_query::ContextListQuery,
    listed_contexts::ListedContexts,
    mutation::ContextMutation,
    pending_hits::fold_pending_hits,
    registry_invariant::RegistryInvariant,
    removal::ContextRemoval,
    served_context::ServedContext,
    store::ContextRegistryStore,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::{compound_context_host::WrapperUse, context_subtree::context_subtree, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::JsonLdContextId};
use andromeda_ngsi_ld_model::jsonld_context::ContextKind;
use andromeda_ngsi_ld_query::page_window::PageWindow;
use iri_rs::IriBuf;
use std::sync::Arc;

/// Manages hosted and cached JSON-LD Contexts independently of persistence.
pub struct ContextRegistryService {
    /// The selected hosted-context persistence implementation.
    store: Arc<dyn ContextRegistryStore>,
    /// JSON-LD resolver used for cache reload and in-process invalidation.
    engine: JsonLdEngine,
    /// The `ImplicitlyCreated` Contexts the broker hosts on its own initiative.
    hosting: ImplicitContextHosting,
}

impl ContextRegistryService {
    /// Create a Context Registry service from persistence and JSON-LD capabilities.
    ///
    /// A stored `ImplicitlyCreated` identifier is remembered for as long as the
    /// engine trusts its copy of a hosted document before reading it again.
    #[must_use]
    pub fn new(store: Arc<dyn ContextRegistryStore>, engine: JsonLdEngine) -> ContextRegistryService {
        let hosting = ImplicitContextHosting::new(Arc::clone(&store), engine.hosted_context_revalidation().as_duration());
        ContextRegistryService { store, engine, hosting }
    }

    /// Create an explicitly hosted Context resource.
    ///
    /// Everything the request carried outside the `@context` subtree is
    /// discarded before the document is stored (clause 5.13.2.3). Clause
    /// 5.13.2.4 makes every Add @context "a new entry", so two creations of the
    /// same document are two entries, each under a fresh random identifier.
    ///
    /// # Errors
    ///
    /// Returns [`ContextRegistryError::NoLocalContext`] when the payload
    /// carries no JSON-LD context in a top-level `@context` member,
    /// [`ContextRegistryError::Persistence`] when the store cannot record the
    /// hosted Context, and [`ContextRegistryError::Invariant`] when a freshly
    /// minted identifier is not one or is already held.
    pub async fn create(&self, payload: &JsonLiteral, locator: &dyn HostedContextLocator) -> Result<JsonLdContextId, ContextRegistryError> {
        let document = hosted_context_subtree(payload)?;
        InsertionPolicy::over(self.store.as_ref(), locator)
            .fresh(&document, ContextCreationKind::Hosted)
            .await
    }

    /// Host a compound Context the broker needs while processing another
    /// resource, and return the identifier it is served under.
    ///
    /// Everything outside the `@context` subtree is discarded before the
    /// document is stored (clause 5.13.2.3), so a Context the broker mints for
    /// its own use is shaped exactly like one a client supplied.
    ///
    /// Equal documents share one entry under an identifier derived from their
    /// content, and a different document is never served under a shared one
    /// (see [`InsertionPolicy::content_derived`]): a thousand subscriptions with
    /// the same `@context` array are served by one stored document, and a
    /// document this process stored recently is not written again for a
    /// response (see [`ImplicitContextHosting`]). `locator` names the URL the
    /// entry is served under, and `usage` what that URL is kept for.
    ///
    /// # Errors
    ///
    /// Returns [`ContextRegistryError::NoLocalContext`] when the payload
    /// carries no JSON-LD context in a top-level `@context` member,
    /// [`ContextRegistryError::Persistence`] when the store cannot record or read
    /// the Context, and [`ContextRegistryError::Invariant`] when an identifier
    /// cannot be built or a freshly minted one is already held.
    pub async fn create_implicitly(
        &self,
        payload: &JsonLiteral,
        locator: &dyn HostedContextLocator,
        usage: WrapperUse,
    ) -> Result<JsonLdContextId, ContextRegistryError> {
        let document = hosted_context_subtree(payload)?;
        self.hosting.host(&document, locator, usage).await
    }

    /// The page `window` selects of the available Contexts, optionally
    /// filtered by lifecycle kind (clause 5.13.3).
    ///
    /// Clause 5.13.3.4 lists "@contexts currently stored", so the embedded
    /// core Context is not among them: it is not a stored entry, and every
    /// reference to it names its ETSI URL (clause 4.4).
    ///
    /// # Errors
    ///
    /// Returns [`ContextRegistryError::Persistence`] when the store cannot read
    /// the stored rows.
    pub async fn list(&self, kind: Option<ContextKind>, detail_level: ContextDetailLevel, window: &PageWindow) -> Result<ContextPage, ContextRegistryError> {
        let counter = self.engine.hit_counter();
        let _flush_held_off = match (&counter, detail_level) {
            (Some(counter), ContextDetailLevel::Full) => Some(counter.read_settled().await),
            (Some(_) | None, ContextDetailLevel::Full | ContextDetailLevel::Summary) => None,
        };
        let mut page = self
            .store
            .list(ContextListQuery {
                kind: kind.as_ref(),
                detail_level,
                window,
            })
            .await?;
        match (&counter, &mut page.contexts) {
            (Some(counter), ListedContexts::Detailed(metadata)) => metadata.iter_mut().for_each(|entry| fold_pending_hits(counter, entry)),
            (Some(_), ListedContexts::Summary(_)) | (None, ListedContexts::Detailed(_) | ListedContexts::Summary(_)) => {}
        }
        Ok(page)
    }

    /// Retrieve a Context document or its lifecycle metadata.
    ///
    /// A `"Hosted"` or `"ImplicitlyCreated"` document comes back with the span
    /// after which this broker reads its row again
    /// ([`JsonLdEngine::hosted_context_revalidation`]), so a party caching the
    /// served copy can be held to the same span.
    ///
    /// # Errors
    ///
    /// Returns [`ContextRegistryError::NotFound`] when no Context is stored under
    /// the identifier, or when the row disappears between its read and its access
    /// update. Returns [`ContextRegistryError::Persistence`] when the store cannot
    /// complete the read or the access update.
    pub async fn get(&self, requested: &ContextIdentifier, detail_level: ContextDetailLevel) -> Result<ServedContext, ContextRegistryError> {
        // Clause 5.13.4.4: "It shall give back ResourceNotFound if the
        // identifier is not found." An identifier that keys no row at all is
        // one such, and is answered before the storage is consulted. The
        // embedded core Context keys no row: its URLs address nothing here.
        let Some(identifier) = requested.stored() else {
            return Err(ContextRegistryError::NotFound {
                identifier: requested.clone(), // clone: the error must own the identifier after this call returns.
            });
        };
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
        let counter = self.engine.hit_counter();
        let _flush_held_off = match (&counter, detail_level) {
            (Some(counter), ContextDetailLevel::Full) => Some(counter.read_settled().await),
            (Some(_) | None, ContextDetailLevel::Full | ContextDetailLevel::Summary) => None,
        };
        let fetched = self.store.get(identifier, detail_level).await?.ok_or_else(|| ContextRegistryError::NotFound {
            identifier: ContextIdentifier::from(identifier.clone()), // clone: the error must own the identifier after this call returns.
        })?;
        if detail_level == ContextDetailLevel::Summary {
            let access = self.store.record_access(identifier).await?;
            if access == ContextMutation::Missing {
                return Err(ContextRegistryError::NotFound {
                    identifier: ContextIdentifier::from(identifier.clone()), // clone: the row disappeared between its read and access update.
                });
            }
        }
        match fetched {
            FetchedContext::Detailed(mut metadata) => {
                if let Some(counter) = &counter {
                    fold_pending_hits(counter, &mut metadata);
                }
                Ok(ServedContext::Metadata(metadata))
            }
            FetchedContext::Payload(document) => Ok(ServedContext::Stored {
                document,
                revalidation: self.engine.hosted_context_revalidation(),
            }),
        }
    }

    /// Delete a Context or reload a cached Context from its source.
    ///
    /// # Errors
    ///
    /// Returns [`ContextRegistryError::NotFound`] when no
    /// Context is stored under the identifier,
    /// [`ContextRegistryError::ReloadRequiresCached`] when reload names a Context
    /// of a kind other than `Cached`, [`ContextRegistryError::MissingSourceUrl`]
    /// when a cached Context records no remote source,
    /// [`ContextRegistryError::ContextUnavailable`] when the remote document
    /// cannot be downloaded, [`ContextRegistryError::Persistence`] when the store
    /// cannot complete the deletion or the replacement, and
    /// [`ContextRegistryError::Invariant`] when the stored identifier is not an
    /// IRI reference the in-process cache can be invalidated by.
    pub async fn remove(&self, requested: &ContextIdentifier, removal: ContextRemoval) -> Result<(), ContextRegistryError> {
        // Clause 5.13.5.4: "If the @context identifier does not correspond to
        // any existing entry in the @context storage, then an error of type
        // ResourceNotFound shall be raised." Only a *missing* identifier is
        // `BadRequestData`, and a path segment is never missing. The embedded
        // core Context keys no row, so its URLs reach this same refusal.
        let Some(identifier) = requested.stored() else {
            return Err(ContextRegistryError::NotFound {
                identifier: requested.clone(), // clone: the error must own the identifier after this call returns.
            });
        };
        match removal {
            ContextRemoval::Delete => self.delete(identifier).await,
            ContextRemoval::Reload => self.reload(identifier).await,
        }
    }

    /// Delete one stored Context and invalidate every in-process lookup key.
    async fn delete(&self, identifier: &Identifier) -> Result<(), ContextRegistryError> {
        let source_url = self.store.lookup(identifier).await?.and_then(|stored| stored.source_url);
        if self.store.delete(identifier).await? == ContextMutation::Missing {
            return Err(ContextRegistryError::NotFound {
                identifier: ContextIdentifier::from(identifier.clone()), // clone: the error must own the identifier after this call returns.
            });
        }
        self.hosting.forget(identifier)?;
        if let Some(source_url) = source_url {
            self.engine.invalidate_context(&source_url).await;
        }
        let identifier_iri = IriBuf::new(identifier.to_string()).map_err(RegistryInvariant::IdentifierNotIri)?;
        self.engine.invalidate_context(&identifier_iri).await;
        Ok(())
    }

    /// Re-download one cached Context and invalidate its prior in-process value.
    async fn reload(&self, identifier: &Identifier) -> Result<(), ContextRegistryError> {
        // Clause 5.13.5.4 orders the two refusals: "If the @context identifier
        // does not correspond to any existing entry in the @context storage,
        // then an error of type ResourceNotFound shall be raised" comes before
        // the kind is considered, because an identifier naming nothing has no
        // kind to object to.
        let stored = self.store.lookup(identifier).await?.ok_or_else(|| ContextRegistryError::NotFound {
            identifier: ContextIdentifier::from(identifier.clone()), // clone: the error must own the identifier after this call returns.
        })?;
        if stored.kind != ContextKind::Cached {
            return Err(ContextRegistryError::ReloadRequiresCached {
                identifier: identifier.clone(), // clone: the error must own the identifier after this call returns.
            });
        }
        let source_url = stored.source_url.ok_or_else(|| ContextRegistryError::MissingSourceUrl {
            identifier: identifier.clone(), // clone: the error must own the identifier after this call returns.
        })?;
        let downloaded = self
            .engine
            .fetch_remote_context(&source_url)
            .await
            .map_err(|error| ContextRegistryError::ContextUnavailable {
                source_url: source_url.clone(), // clone: the source is also needed to invalidate the previous document.
                source: Box::new(error),
            })?;
        // Clause 5.13.5.4: "the existing @context is replaced with the newly
        // downloaded one." The resolver hands the download back already reduced
        // to its `@context` subtree (clause 5.13.2.3), which is exactly the
        // shape a stored Context has, so it is stored as it stands, together
        // with the URL it was finally read from: the source may redirect
        // elsewhere than it did at the first download. Wrapping it in a second
        // `@context` would give a document no expansion can use, and clause
        // 5.13.5 offers no operation that could undo that.
        if self.store.replace_download(&stored.identifier, &downloaded).await? == ContextMutation::Missing {
            return Err(ContextRegistryError::NotFound {
                identifier: ContextIdentifier::from(stored.identifier),
            });
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
/// Returns [`ContextRegistryError::NoLocalContext`] when the payload is not a
/// JSON object, is one that carries no top-level `@context` member, or carries
/// one that holds no JSON-LD context.
fn hosted_context_subtree(payload: &JsonLiteral) -> Result<JsonLiteral, ContextRegistryError> {
    context_subtree(payload).map_err(ContextRegistryError::NoLocalContext)
}

#[cfg(test)]
mod core_url_tests;

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod identity_tests;

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod memory_store;

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{error::ContextRegistryError, service::hosted_context_subtree};
    use andromeda_json_literal::JsonLiteral;
    use serde_json::json;

    /// Clause 5.13.2.3: "all extra information located outside of the @context
    /// subtree in the referenced object shall be discarded". What survives the
    /// reduction is asserted where the reduction lives; what this owns is that
    /// a hosted document goes through it at all.
    #[test]
    fn a_hosted_document_keeps_only_its_context_subtree() -> Result<(), ContextRegistryError> {
        let supplied = json!({
            "@context": { "Building": "https://example.com/Building" },
            "id": "urn:ngsi-ld:JsonLdContext:supplied-by-the-client",
            "createdAt": "2020-01-01T00:00:00Z",
            "extraInfo": { "owner": "someone" }
        });

        pretty_assertions::assert_eq!(
            hosted_context_subtree(&JsonLiteral::admit(&supplied.to_string()).expect("admissible"))?,
            JsonLiteral::admit(&json!({ "@context": { "Building": "https://example.com/Building" } }).to_string()).expect("admissible")
        );
        Ok(())
    }

    /// Clause 5.13.2.3 requires a top-level `@context` field holding a local
    /// context; a client document without one names no local context, which is
    /// a request error.
    #[test]
    fn a_document_without_a_top_level_context_is_refused() {
        for supplied in [
            json!({ "A1": "urn:ngsi-ld:attributes:A1" }),
            json!({}),
            json!([{ "@context": {} }]),
            json!("@context"),
            json!({ "@context": 5 }),
        ] {
            assert!(
                matches!(
                    hosted_context_subtree(&JsonLiteral::admit(&supplied.to_string()).expect("admissible")),
                    Err(ContextRegistryError::NoLocalContext(_))
                ),
                "a document with no top-level @context must be refused"
            );
        }
    }
}
