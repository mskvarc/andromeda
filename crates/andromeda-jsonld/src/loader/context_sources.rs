//! The documents a processed context was built from.
//!
//! Processing a `@context` resolves every document it names, and the active
//! context it yields is a function of exactly those documents. A processed
//! context kept across operations is therefore only as fresh as the documents
//! the loader handed out while it was processed: once the loader's entry for
//! one of them lapses (clause 6.3.16 for a download, the revalidation span for
//! a document the broker stores itself) and the document is read again, the
//! processed copy is stale.
//!
//! [`recording_sources`] runs a preparation while noting every document the
//! loader resolves for it, and [`ContextSources::still_resolve_through`]
//! resolves the same references again and tells whether every one still
//! yields the very document it yielded then. Resolving them again, rather than
//! peeking at the loader's cache, keeps the clause 5.13.3.5 access accounting
//! of a reused context the same as that of a processed one.
//!
//! Recording is task-local, like the operation scope of
//! [`crate::operation_hit_scope`], because the resolutions happen deep inside
//! the JSON-LD processor, which knows nothing about the cache it is feeding.

use crate::{engine::context_document::ContextDocument, loader::http::HttpContextLoader};
use iri_rs::IriBuf;
use std::{cell::RefCell, future::Future, sync::Arc};

tokio::task_local! {
    /// The documents resolved so far by the preparation running on this task.
    static RECORDED_SOURCES: RefCell<Vec<ContextSource>>;
}

/// One document the loader resolved for a preparation.
#[derive(Debug, Clone)]
struct ContextSource {
    /// The reference the document was resolved under.
    url: IriBuf,
    /// The document the loader handed out, compared by identity.
    document: Arc<ContextDocument>,
}

/// Every document one preparation resolved, each reference once.
#[derive(Debug, Clone, Default)]
pub(crate) struct ContextSources(Vec<ContextSource>);

impl ContextSources {
    /// Whether resolving every recorded reference again yields the very
    /// document it yielded while the context was prepared.
    ///
    /// A reference that no longer resolves makes the context stale too, so
    /// preparing it again reports the failure where it now occurs.
    pub(crate) async fn still_resolve_through(&self, loader: &HttpContextLoader) -> bool {
        for source in &self.0 {
            match loader.load_document_raw(&source.url).await {
                Ok(document) if Arc::ptr_eq(&document, &source.document) => {}
                Ok(_) | Err(_) => return false,
            }
        }
        true
    }

    /// How many distinct documents were recorded.
    #[cfg(test)]
    pub(crate) const fn len(&self) -> usize {
        self.0.len()
    }
}

/// Run `preparation`, recording every document the loader resolves for it.
pub(crate) async fn recording_sources<F>(preparation: F) -> (F::Output, ContextSources)
where
    F: Future,
{
    RECORDED_SOURCES
        .scope(RefCell::new(Vec::new()), async {
            let output = preparation.await;
            let sources = RECORDED_SOURCES.with(RefCell::take);
            (output, ContextSources(sources))
        })
        .await
}

/// Note that the loader resolved `url` to `document`, when a preparation on
/// this task is recording; a no-op otherwise.
pub(crate) fn note_source(url: &IriBuf, document: &Arc<ContextDocument>) {
    let noted = RECORDED_SOURCES.try_with(|recorded| {
        let mut recorded = recorded.borrow_mut();
        if recorded.iter().any(|source| source.url == *url) {
            return;
        }
        recorded.push(ContextSource {
            url: url.clone(),               // clone: the record owns the reference it resolves again
            document: Arc::clone(document), // clone: Arc ref-count bump, the identity later resolutions are compared with
        });
    });
    match noted {
        Ok(()) => {}
        // An access outside a recording preparation records nothing.
        Err(_outside_a_recording) => {}
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        context_cache::ContextCache,
        context_registry::ContextRegistry,
        hosted_context_recognizer::NoHostedContexts,
        loader::{context_sources::recording_sources, http::HttpContextLoader, loader_settings::LoaderSettings, replaceable_row::ReplaceableRow},
        test_documents::document_value,
    };
    use iri_rs::IriBuf;
    use serde_json::json;
    use std::sync::Arc;

    fn loader_over(row: &Arc<ReplaceableRow>) -> HttpContextLoader {
        HttpContextLoader::new(Arc::new(ContextRegistry::new()), LoaderSettings::offline(), Arc::new(NoHostedContexts))
            .expect("the loader builds")
            .with_l2_cache(Arc::clone(row) as Arc<dyn ContextCache>)
    }

    fn url() -> IriBuf {
        IriBuf::new("urn:ngsi-ld:JsonLdContext:recorded".to_owned()).expect("a valid identifier")
    }

    #[tokio::test]
    async fn every_resolution_of_a_preparation_is_recorded_once_per_reference() {
        let row = Arc::new(ReplaceableRow::new(&json!({ "@context": { "speed": "https://example.org/speed" } })));
        let loader = loader_over(&row);

        let (resolved, sources) = Box::pin(recording_sources(async {
            loader.load_document(&url()).await.expect("the first resolution");
            loader.load_document(&url()).await.expect("the second resolution")
        }))
        .await;

        assert!(document_value(&resolved).is_object());
        pretty_assertions::assert_eq!(sources.len(), 1);
        assert!(sources.still_resolve_through(&loader).await, "the loader still holds the recorded document");
    }

    #[tokio::test]
    async fn a_resolution_outside_a_recording_records_nothing() {
        let row = Arc::new(ReplaceableRow::new(&json!({ "@context": { "speed": "https://example.org/speed" } })));
        let loader = loader_over(&row);
        loader.load_document(&url()).await.expect("a resolution outside any recording");

        let ((), sources) = recording_sources(async {}).await;

        pretty_assertions::assert_eq!(sources.len(), 0);
    }

    /// Once the loader's entry lapses and the document is read again, the
    /// recorded document is no longer the one the reference resolves to.
    #[tokio::test]
    async fn a_document_read_again_no_longer_resolves_through_the_recorded_sources() {
        let row = Arc::new(ReplaceableRow::new(&json!({ "@context": { "speed": "https://example.org/speed" } })));
        let loader = loader_over(&row);
        let ((), sources) = Box::pin(recording_sources(async {
            loader.load_document(&url()).await.expect("the recorded resolution");
        }))
        .await;

        row.replace(&json!({ "@context": { "speed": "https://example.org/velocity" } }));
        loader.invalidate(&url()).await;

        assert!(!sources.still_resolve_through(&loader).await, "the document was read again");
    }
}
