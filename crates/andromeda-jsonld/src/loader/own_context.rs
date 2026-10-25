//! Resolving a reference to one of the tenant's own stored `@context`s.
//!
//! Clause 5.13.1 serves Hosted and `ImplicitlyCreated` `@context`s "on-demand"
//! from the broker's storage. A request, a stored subscription or a context
//! document may name one by the URL it is served under, on whatever origin the
//! name was built with. The loader resolves such a reference from the tenant's
//! storage by its identifier, which is what a JSON-LD processor fetching the URL
//! would receive, without a request to the broker itself.
//!
//! Which references are shaped like the tenant's own is the loader's
//! [`crate::hosted_context_recognizer::HostedContextRecognizer`]'s to say. A
//! reference so shaped whose identifier the tenant does not hold may be another
//! broker's, so it is resolved as the URL it is; the download is still never
//! recorded as a "Cached" copy, so the broker holds no Cached row under a URL
//! shaped like one of its own.

use crate::loader::{
    durable_cache::load_stored_context,
    error::LoaderError,
    http::HttpContextLoader,
    loaded_context::{ContextProvenance, LoadedContext},
};
use iri_rs::IriBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Whether a resolution may record the document it downloads as a "Cached"
/// durable entry (clause 5.13.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CachedCopy {
    /// The durable cache is read and a first download is recorded in it.
    Recorded,
    /// The durable cache is neither read nor written: the URL is shaped like one
    /// of the broker's own, which it never holds a Cached copy of.
    Forbidden,
}

/// What the tenant's storage holds for a reference to one of its own
/// `@context`s.
#[derive(Debug)]
pub(crate) enum StoredLookup {
    /// The tenant holds the identifier as a Hosted or `ImplicitlyCreated` entry.
    ///
    /// Boxed: the entry is several times the size of the other variant, and a
    /// lookup is returned once per resolution, not stored.
    Held(Box<LoadedContext>),
    /// It does not; the reference is resolved as the URL it is.
    NotHeld,
}

/// Why the in-process cache was not filled for a stored identifier.
#[derive(Debug)]
enum StoredMiss {
    /// The tenant holds no Hosted or `ImplicitlyCreated` entry under it.
    NotHeld,
    /// The durable cache could not be read.
    Failed(LoaderError),
}

impl HttpContextLoader {
    /// Resolve the stored `@context` `identifier` names from the tenant's
    /// storage, through the in-process cache.
    ///
    /// The in-process entry is keyed by the identifier, which is also the key a
    /// request naming the identifier itself resolves under, so both spellings
    /// share one entry and one invalidation. Only a document the broker stores
    /// itself is held: a "Cached" download a request reached by its local
    /// identifier may occupy the same key, and clause 5.13.1 never serves one.
    ///
    /// # Errors
    ///
    /// Returns the [`LoaderError`] the durable read fails with when remote
    /// fetch is denied.
    pub(super) async fn load_stored(&self, identifier: &IriBuf) -> Result<StoredLookup, Arc<LoaderError>> {
        if let Some(loaded) = self.cache.get(identifier).await {
            return Ok(match loaded.provenance {
                ContextProvenance::Broker => {
                    self.record_cached_access(identifier, &loaded).await;
                    StoredLookup::Held(Box::new(loaded))
                }
                ContextProvenance::Local | ContextProvenance::Downloaded(_) => StoredLookup::NotHeld,
            });
        }
        let Some(ref l2_cache) = self.l2_cache else {
            return Ok(StoredLookup::NotHeld);
        };
        let l2_cache = Arc::clone(l2_cache); // clone: Arc ref-count bump into the cache-fill future
        let key = identifier.clone(); // owned: the moka key and the cache-fill future both need it
        let allow_remote = self.allow_remote;
        let hosted_revalidation = self.lifetimes.hosted_revalidation;
        let filled_here = Arc::new(AtomicBool::new(false));
        let fill_marker = Arc::clone(&filled_here); // clone: the cache-fill future records whether this caller filled the entry

        let result = self
            .cache
            .try_get_with(key.clone(), async move {
                fill_marker.store(true, Ordering::Relaxed);
                match load_stored_context(l2_cache.as_ref(), &key, allow_remote, hosted_revalidation).await {
                    Ok(Some(loaded)) => Ok(loaded),
                    Ok(None) => Err(StoredMiss::NotHeld),
                    Err(failed) => Err(StoredMiss::Failed(failed)),
                }
            })
            .await;

        match result {
            Ok(loaded) => {
                self.settle_access(identifier, &loaded, &filled_here).await;
                Ok(StoredLookup::Held(Box::new(loaded)))
            }
            Err(miss) => match miss.as_ref() {
                StoredMiss::NotHeld => Ok(StoredLookup::NotHeld),
                StoredMiss::Failed(failed) => Err(Arc::new(failed.clone())), // clone: moka shares the error between every waiter
            },
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        context_cache::{CachedContext, ContextCache, ContextCacheError, StoredDocumentSource},
        context_registry::ContextRegistry,
        error::JsonLdError,
        hosted_context_recognizer::{HostedContextRecognizer, HostedReference},
        loader::{
            cache_lifetime::{CacheLifetime, ResidualLifetime},
            http::HttpContextLoader,
            loader_settings::LoaderSettings,
        },
        test_documents::{document_value, literal},
    };
    use andromeda_json_literal::JsonLiteral;
    use iri_rs::{IriBuf, IriRefBuf};
    use serde_json::{Value, json};
    use std::{
        collections::HashMap,
        sync::{
            Arc,
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    /// The path every stored `@context` is served under.
    const CONTEXTS_PATH: &str = "/ngsi-ld/v1/jsonldContexts/";

    /// The identifier the tenant holds as a Hosted entry.
    const HELD: &str = "urn:ngsi-ld:JsonLdContext:6f1c0e2a-3b5d-4e7f-8a9b-0c1d2e3f4a5b";

    /// An identifier the tenant does not hold.
    const UNHELD: &str = "urn:ngsi-ld:JsonLdContext:0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

    /// The identifier the tenant holds as a "Cached" download.
    const CACHED: &str = "urn:ngsi-ld:JsonLdContext:1f2e3d4c-5b6a-4978-8a9b-cdef01234567";

    /// Recognises every reference whose path ends in the stored-context path
    /// followed by one identifier, on any origin.
    #[derive(Debug)]
    struct OwnPaths;

    impl HostedContextRecognizer for OwnPaths {
        fn recognize(&self, reference: &IriRefBuf) -> HostedReference {
            match reference.as_str().rsplit_once(CONTEXTS_PATH) {
                Some((_, identifier)) => identifier.parse().map_or(HostedReference::Unrelated, HostedReference::Own),
                None => HostedReference::Unrelated,
            }
        }
    }

    /// A durable cache holding fixed rows by key and recording every read and
    /// write.
    #[derive(Debug, Default)]
    struct StoredRows {
        /// The rows, by the key a read names.
        rows: HashMap<String, CachedContext>,
        /// How many reads reached the cache.
        reads: AtomicUsize,
        /// The URLs handed to `put`, in call order.
        puts: Mutex<Vec<String>>,
    }

    impl StoredRows {
        /// Hold `payload` under `key`, with `residual` left of its lifetime.
        fn holding(mut self, key: &str, payload: &Value, residual: ResidualLifetime) -> StoredRows {
            // A row with no expiry is one the broker stores itself; any other
            // is a download of the key it is held under.
            let source = match residual {
                ResidualLifetime::Unbounded => StoredDocumentSource::Broker,
                ResidualLifetime::Bounded(_) => StoredDocumentSource::Downloaded {
                    document_url: IriBuf::new(key.to_owned()).expect("a fixture key is an IRI"),
                },
            };
            self.rows.insert(
                key.to_owned(),
                CachedContext {
                    payload: literal(payload),
                    residual,
                    canonical_url: None,
                    source,
                },
            );
            self
        }

        fn puts(&self) -> Vec<String> {
            self.puts.lock().expect("the lock is not poisoned").clone() // clone: the assertion owns a snapshot
        }
    }

    #[async_trait::async_trait]
    impl ContextCache for StoredRows {
        async fn get(&self, url: &IriBuf) -> Result<Option<CachedContext>, ContextCacheError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(self.rows.get(url.as_str()).cloned()) // clone: the fixture keeps its row for the next read
        }

        async fn put(&self, url: &IriBuf, _document_url: &IriBuf, _payload: &JsonLiteral, _lifetime: CacheLifetime) -> Result<(), ContextCacheError> {
            self.puts.lock().expect("the lock is not poisoned").push(url.to_string());
            Ok(())
        }
    }

    /// A loader that may not reach the network, over `registry` and `rows`,
    /// recognising [`OwnPaths`].
    fn loader(registry: Arc<ContextRegistry>, rows: &Arc<StoredRows>) -> HttpContextLoader {
        HttpContextLoader::new(registry, LoaderSettings::offline(), Arc::new(OwnPaths))
            .expect("the loader builds")
            .with_l2_cache(Arc::clone(rows) as Arc<dyn ContextCache>)
    }

    fn iri(text: String) -> IriBuf {
        IriBuf::new(text).expect("a valid IRI")
    }

    fn terms() -> Value {
        json!({ "@context": { "speed": "https://example.org/speed" } })
    }

    /// Clause 5.13.1: a held entry is served from storage whatever origin its
    /// URL names, and the network is never tried (fetching is denied here).
    #[tokio::test]
    async fn a_held_identifier_resolves_from_storage_on_any_origin() {
        let rows = Arc::new(StoredRows::default().holding(HELD, &terms(), ResidualLifetime::Unbounded));
        let loader = loader(Arc::new(ContextRegistry::new()), &rows);

        for origin in ["http://127.0.0.1:1056", "https://gateway.example.com/prefix"] {
            let value = loader
                .load_document(&iri(format!("{origin}{CONTEXTS_PATH}{HELD}")))
                .await
                .expect("the held entry resolves from storage");
            pretty_assertions::assert_eq!(document_value(&value), terms());
        }
        pretty_assertions::assert_eq!(rows.reads.load(Ordering::SeqCst), 1, "both spellings share one in-process entry");
        assert!(rows.puts().is_empty(), "no Cached row is recorded for the broker's own document");
    }

    /// A reference shaped like one of the tenant's own whose identifier the
    /// tenant does not hold may be another broker's: it is resolved as the URL
    /// it is, so the network step is reached for that URL rather than the
    /// identifier.
    #[tokio::test]
    async fn an_unheld_identifier_is_resolved_as_the_url_it_is() {
        let rows = Arc::new(StoredRows::default());
        let loader = loader(Arc::new(ContextRegistry::new()), &rows);
        let requested = iri(format!("https://other-broker.example{CONTEXTS_PATH}{UNHELD}"));

        let error = loader.load_document(&requested).await.expect_err("fetching is denied");

        assert!(matches!(&error, JsonLdError::ContextFetchDisabled { url } if *url == requested), "{error}");
    }

    /// Clause 5.13.1 serves a "Cached" entry never, so its local identifier
    /// named as a URL is not resolved from storage either.
    #[tokio::test]
    async fn a_cached_entry_is_not_served_from_storage_by_its_local_identifier() {
        let bounded = ResidualLifetime::Bounded(CacheLifetime::from_seconds(60).expect("a minute is a positive lifetime"));
        let rows = Arc::new(StoredRows::default().holding(CACHED, &terms(), bounded));
        let loader = loader(Arc::new(ContextRegistry::new()), &rows);
        let requested = iri(format!("http://127.0.0.1:1056{CONTEXTS_PATH}{CACHED}"));

        let error = loader.load_document(&requested).await.expect_err("fetching is denied");

        assert!(matches!(&error, JsonLdError::ContextFetchDisabled { url } if *url == requested), "{error}");
    }

    /// A download of a URL shaped like one of the broker's own is never
    /// recorded as a "Cached" row: here the document comes from the registry,
    /// which would otherwise be written to the durable cache.
    #[tokio::test]
    async fn a_url_shaped_like_an_own_url_is_never_recorded_as_cached() {
        let registry = Arc::new(ContextRegistry::new());
        let own_shaped = iri(format!("https://other-broker.example{CONTEXTS_PATH}{UNHELD}"));
        registry.register_preloaded(own_shaped.clone(), literal(&terms())).expect("registered"); // clone: the load below names the same URL
        let rows = Arc::new(StoredRows::default());
        let loader = loader(registry, &rows);

        loader.load_document(&own_shaped).await.expect("the preloaded document resolves");

        assert!(rows.puts().is_empty(), "no Cached row is recorded under a URL shaped like the broker's own");
    }

    /// A "Cached" download a request reached by its local identifier occupies
    /// the identifier's in-process key; it is not served as one of the tenant's
    /// own stored contexts (clause 5.13.1 never serves a Cached entry).
    #[tokio::test]
    async fn a_cached_download_under_an_identifier_key_is_not_served_as_an_own_context() {
        let bounded = ResidualLifetime::Bounded(CacheLifetime::from_seconds(60).expect("a minute is a positive lifetime"));
        let rows = Arc::new(StoredRows::default().holding(CACHED, &terms(), bounded));
        let loader = loader(Arc::new(ContextRegistry::new()), &rows);
        loader
            .load_document(&iri(CACHED.to_owned()))
            .await
            .expect("the bare identifier resolves the Cached row as the URL it is");

        let error = loader
            .load_document(&iri(format!("http://127.0.0.1:1056{CONTEXTS_PATH}{CACHED}")))
            .await
            .expect_err("the Cached entry is not served under the stored-context path, and fetching is denied");

        assert!(matches!(&error, JsonLdError::ContextFetchDisabled { .. }), "{error}");
    }

    /// The same preloaded document under an ordinary URL is recorded, which is
    /// what the refusal above departs from.
    #[tokio::test]
    async fn an_ordinary_preloaded_url_is_still_recorded_as_cached() {
        let registry = Arc::new(ContextRegistry::new());
        let ordinary = iri("https://example.org/terms.jsonld".to_owned());
        registry.register_preloaded(ordinary.clone(), literal(&terms())).expect("registered"); // clone: the load below names the same URL
        let rows = Arc::new(StoredRows::default());
        let loader = loader(registry, &rows);

        loader.load_document(&ordinary).await.expect("the preloaded document resolves");

        pretty_assertions::assert_eq!(rows.puts(), vec![ordinary.to_string()]);
    }
}
