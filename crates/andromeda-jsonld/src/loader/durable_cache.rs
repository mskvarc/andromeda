//! The durable-cache steps of context resolution.
//!
//! Between the in-process cache and the network sits the durable cache a broker
//! attaches — the rows clause 5.13.1 flags `"Cached"`. Reading one is a hit;
//! writing one happens exactly once per URL, on the first remote-fetch success,
//! and carries the lifetime clause 6.3.16 derived from that response so the row
//! lapses when the origin server said it would.
//!
//! A hit carries back what is left of that lifetime, so the in-process tier is
//! held to the same instant rather than being re-granted the full heuristic
//! span every time the durable row is read.

use crate::{
    context_cache::{CachedContext, ContextCache, StoredDocumentSource},
    context_registry::RegisteredContext,
    core_context_precedence::{PreparedContext, prepared_context},
    engine::options::RemoteContextFetch,
    loader::{
        cache_lifetime::{CacheLifetime, ContextFreshness},
        error::{ContextUnavailable, LoaderError, LoaderErrorKind},
        loaded_context::{ContextDurability, ContextProvenance, EntryLifetime, LoadedContext},
        pending_persistence::PendingPersistence,
        relative_context_references::resolve_relative_context_references,
        remote_document::fetch_and_parse_retrying,
    },
};
use iri_rs::IriBuf;
use reqwest::Client;
use std::sync::Arc;
use tracing::{trace, warn};

/// The durable-cache step, yielding a context when the cache holds a fresh one.
///
/// A read failure is fatal only when configuration forbids the network. With
/// remote fetch allowed the failure is logged and resolution continues to the
/// source, because a degraded cache must not make a reachable context
/// unresolvable.
///
/// The entry the hit produces is bounded by what is left of the stored row's
/// lifetime, which the hit carries. Clause 6.3.16 makes the span the origin
/// server granted authoritative over every tier holding the document, so the
/// in-process copy lapses at the instant the row does — bounding it by the
/// configured heuristic instead would keep serving, for an hour, a `@context`
/// the origin server said lives thirty seconds. A row with no expiry is one the
/// broker stores itself, and its copy is bounded by `hosted_revalidation`
/// instead (see [`EntryLifetime::of_residual`]).
///
/// # Errors
///
/// Returns [`ContextUnavailable::Cache`] when the durable read fails and remote
/// fetch is denied.
pub(crate) async fn load_from_durable_cache(
    cache: &dyn ContextCache,
    url: &IriBuf,
    allow_remote: RemoteContextFetch,
    hosted_revalidation: CacheLifetime,
) -> Result<Option<LoadedContext>, LoaderError> {
    Ok(read_durable_cache(cache, url, allow_remote)
        .await?
        .map(|hit| durable_hit(url, hit, hosted_revalidation)))
}

/// The durable-cache step for one of the tenant's own stored `@context`s,
/// yielding it only when the tenant holds it as a Hosted or `ImplicitlyCreated`
/// entry.
///
/// Clause 5.13.1 serves those two kinds "on-demand" from storage; a "Cached"
/// entry is never served, so a reference to its local identifier is not
/// resolved from storage either. The row of a document the broker stores
/// itself reports [`StoredDocumentSource::Broker`], which is what tells it from
/// a download.
///
/// # Errors
///
/// Returns [`ContextUnavailable::Cache`] when the durable read fails and remote
/// fetch is denied.
pub(crate) async fn load_stored_context(
    cache: &dyn ContextCache,
    identifier: &IriBuf,
    allow_remote: RemoteContextFetch,
    hosted_revalidation: CacheLifetime,
) -> Result<Option<LoadedContext>, LoaderError> {
    Ok(read_durable_cache(cache, identifier, allow_remote)
        .await?
        .filter(|hit| hit.source == StoredDocumentSource::Broker)
        .map(|hit| durable_hit(identifier, hit, hosted_revalidation)))
}

/// Read the durable cache, degrading a failed read to a miss when the network
/// may still be tried.
async fn read_durable_cache(cache: &dyn ContextCache, url: &IriBuf, allow_remote: RemoteContextFetch) -> Result<Option<CachedContext>, LoaderError> {
    match cache.get(url).await {
        Ok(hit) => Ok(hit),
        Err(degraded) if allow_remote == RemoteContextFetch::Allow => {
            warn!(url = %url, error = %degraded, "Durable context-cache read failed; attempting remote resolution");
            Ok(None)
        }
        Err(unavailable) => Err(LoaderError {
            url: url.clone(), // clone: resolution continues to report under this URL
            kind: LoaderErrorKind::Unavailable(ContextUnavailable::Cache(unavailable)),
        }),
    }
}

/// The in-process entry a durable-cache hit under `url` produces.
///
/// A downloaded document keeps the URL it was finally read from, which the row
/// stores, so its relative references resolve exactly as they did when it was
/// first downloaded -- after a restart or an eviction as much as before. A
/// document the broker stores itself was never read from anywhere, and its
/// references resolve against the URL a request names it by
/// ([`LoadedContext::tail_from`]), which is also the base the processor reads
/// it against.
fn durable_hit(url: &IriBuf, hit: CachedContext, hosted_revalidation: CacheLifetime) -> LoadedContext {
    trace!("L2 cache hit for context: {url}");
    let (prepared, provenance) = match hit.source {
        StoredDocumentSource::Downloaded { document_url } => (prepared_download(&document_url, hit.payload), ContextProvenance::Downloaded(document_url)),
        StoredDocumentSource::Broker => (prepared_context(url, hit.payload), ContextProvenance::Broker),
    };
    LoadedContext {
        value: Arc::new(prepared.value),
        tail: prepared.tail,
        durability: ContextDurability::Durable,
        lifetime: EntryLifetime::of_residual(hit.residual, hosted_revalidation),
        canonical_url: hit.canonical_url,
        provenance,
    }
}

/// The remote step: fetch the context and, on a first success, persist it.
///
/// The durable row holds the document as downloaded, not the copy prepared for
/// processing: a clause 5.13.1 "Cached" entry is the broker's copy of what the
/// URL served, and what that document leaves in force last (clause 4.4) has to
/// be readable again from the row once the in-process entry is gone. A hit
/// prepares it again on the way out.
///
/// # Errors
///
/// Returns whatever [`fetch_and_parse_retrying`] reports; a failed durable write
/// is not an error, only a downgrade to [`ContextDurability::Pending`].
pub(crate) async fn fetch_and_persist(
    client: &Client,
    url: &IriBuf,
    max_bytes: usize,
    default_lifetime: CacheLifetime,
    l2_cache: Option<&Arc<dyn ContextCache>>,
) -> Result<LoadedContext, LoaderError> {
    let fetched = fetch_and_parse_retrying(client, url, max_bytes, default_lifetime).await?;
    let lifetime = EntryLifetime::of(fetched.freshness);
    let downloaded = jsonld_syntax::Value::into_serde_json(fetched.value);
    let ContextFreshness::Fresh(granted) = fetched.freshness else {
        // Clause 6.3.16: `no-store`, `no-cache` and an already-lapsed expiry all
        // leave nothing to invalidate later, so the document is served once and
        // never stored in either tier.
        let prepared = prepared_download(&fetched.document_url, downloaded);
        return Ok(LoadedContext {
            value: Arc::new(prepared.value),
            tail: prepared.tail,
            durability: ContextDurability::Registry,
            lifetime,
            canonical_url: None,
            provenance: ContextProvenance::Downloaded(fetched.document_url),
        });
    };
    // The document's own references resolve against where it was finally
    // read from, after redirects and an alternate, while the row is keyed by
    // the URL the request named.
    let prepared = prepared_download(&fetched.document_url, downloaded.clone()); // clone: the row keeps the document as downloaded while the processor reads the prepared copy
    // Written to L2 only here, on the first remote-fetch success: the row
    // carries no counter, because hit accounting is the HitCounter's job.
    let durability = match l2_cache {
        Some(cache) => persist_to_durable_cache(cache.as_ref(), url, &fetched.document_url, downloaded, granted).await,
        None => ContextDurability::Registry,
    };
    Ok(LoadedContext {
        value: Arc::new(prepared.value),
        tail: prepared.tail,
        durability,
        lifetime,
        canonical_url: None,
        provenance: ContextProvenance::Downloaded(fetched.document_url),
    })
}

/// Prepare a downloaded document for processing, its relative context
/// references resolved against `document_url`, the URL it was finally read
/// from (JSON-LD 1.1 API section 4.1.2 step 5.2.6; see
/// [`resolve_relative_context_references`]).
fn prepared_download(document_url: &IriBuf, mut downloaded: serde_json::Value) -> PreparedContext {
    resolve_relative_context_references(&mut downloaded, document_url);
    prepared_context(document_url, downloaded)
}

/// Record a preloaded registry context as a "Cached" durable entry.
///
/// Clause 5.13.1 flags "Cached" the copies the broker holds of documents whose
/// authority is a URL it resolved. A preloaded context is a local stand-in for
/// exactly such a download, so the first resolution that reaches the registry
/// writes it to the durable cache and thereafter clause 5.13.3 lists it under
/// its URL. The write lands inside the caller's moka single-flight, so it
/// happens once per URL per process rather than per expansion; a failed write
/// downgrades to [`ContextDurability::Pending`] so a later access retries it.
///
/// A preloaded document has no origin server to grant a lifetime, so the
/// in-process entry is bounded by the resolver's heuristic (`lifetime`); once it
/// lapses the resolver re-hits the registry and refreshes the row. It stands in
/// for a download of `url` itself, so `url` is also where the row records it was
/// read from, and the row holds `as_written`, the document as the operator
/// wrote it, exactly as a download's row holds the document as downloaded: what
/// it leaves in force last is read from that copy again once the in-process
/// entry is gone.
pub(crate) async fn persist_registry_context(
    cache: &dyn ContextCache,
    url: &IriBuf,
    registered: RegisteredContext,
    as_written: &jsonld_syntax::Value,
    lifetime: CacheLifetime,
) -> LoadedContext {
    let payload = jsonld_syntax::Value::into_serde_json(as_written.clone()); // clone: the registry keeps the document as written for the next refresh
    let durability = persist_to_durable_cache(cache, url, url, payload, lifetime).await;
    LoadedContext {
        value: registered.value,
        tail: registered.tail,
        durability,
        lifetime: EntryLifetime::Bounded(lifetime),
        canonical_url: None,
        provenance: ContextProvenance::Local,
    }
}

/// Persist a context document, classifying the entry by whether the durable
/// write landed.
///
/// A write failure keeps the document serving from the in-process cache and
/// hands the payload to [`PendingPersistence`], so the next access retries it
/// rather than losing the row.
async fn persist_to_durable_cache(
    cache: &dyn ContextCache,
    url: &IriBuf,
    document_url: &IriBuf,
    payload: serde_json::Value,
    lifetime: CacheLifetime,
) -> ContextDurability {
    match cache.put(url, document_url, &payload, lifetime).await {
        Ok(()) => {
            trace!("Persisted context to L2 cache: {url}");
            ContextDurability::Durable
        }
        Err(unwritten) => {
            warn!(url = %url, error = %unwritten, "Serving remotely loaded context while durable persistence is pending");
            // clone: the owed write keeps its own copy of where the document was read from
            ContextDurability::Pending(Arc::new(PendingPersistence::owed(payload, document_url.clone(), lifetime)))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_cache::{CachedContext, StoredDocumentSource},
        context_tail::ContextTail,
        loader::{
            cache_lifetime::{CacheLifetime, ResidualLifetime},
            durable_cache::{durable_hit, fetch_and_persist},
            loaded_context::LoadedContext,
            test_origin::{TestOrigin, ok, redirect},
        },
    };
    use iri_rs::IriBuf;
    use reqwest::Client;
    use serde_json::{Value, json};

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_owned()).expect("a valid IRI")
    }

    fn revalidation() -> CacheLifetime {
        CacheLifetime::from_seconds(60).expect("a minute is a positive lifetime")
    }

    /// The document an entry hands the processor, as JSON.
    fn processed(loaded: &LoadedContext) -> Value {
        jsonld_syntax::Value::into_serde_json((*loaded.value).clone()) // clone: the entry is inspected, not consumed
    }

    /// A download read back from its durable row resolves its relative
    /// references against the URL it was finally read from, as the first
    /// download did, not against the URL the row is keyed by.
    #[test]
    fn a_downloaded_row_is_based_on_the_url_it_was_finally_read_from() {
        let requested = iri("https://origin.example.com/moved.jsonld");
        let final_url = iri("https://origin.example.com/v2/wrapper.jsonld");
        let hit = CachedContext {
            payload: json!({ "@context": "terms.jsonld" }),
            residual: ResidualLifetime::Bounded(CacheLifetime::from_seconds(30).expect("thirty seconds is a positive lifetime")),
            canonical_url: None,
            source: StoredDocumentSource::Downloaded {
                document_url: final_url.clone(), // clone: the expectation compares against it
            },
        };

        let loaded = durable_hit(&requested, hit, revalidation());

        pretty_assertions::assert_eq!(loaded.document_url(&requested), &final_url);
        pretty_assertions::assert_eq!(
            loaded.tail_from(&requested),
            ContextTail::Reference(iri("https://origin.example.com/v2/terms.jsonld"))
        );
        pretty_assertions::assert_eq!(processed(&loaded), json!({ "@context": "https://origin.example.com/v2/terms.jsonld" }));
    }

    /// A document the broker stores itself was never read from anywhere, so
    /// its references resolve against the URL a request names it by, not the
    /// identifier the row is keyed by: the tail is kept relative until then.
    #[test]
    fn a_row_the_broker_stores_itself_is_based_on_the_url_a_request_names_it_by() {
        let identifier = iri("urn:ngsi-ld:JsonLdContext:6f1c0e2a-3b5d-4e7f-8a9b-0c1d2e3f4a5b");
        let requested = iri("https://broker.example.com/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:6f1c0e2a-3b5d-4e7f-8a9b-0c1d2e3f4a5b");
        let hit = CachedContext {
            payload: json!({ "@context": "terms.jsonld" }),
            residual: ResidualLifetime::Unbounded,
            canonical_url: None,
            source: StoredDocumentSource::Broker,
        };

        let loaded = durable_hit(&identifier, hit, revalidation());

        pretty_assertions::assert_eq!(loaded.document_url(&requested), &requested);
        pretty_assertions::assert_eq!(
            loaded.tail_from(&requested),
            ContextTail::Reference(iri("https://broker.example.com/ngsi-ld/v1/jsonldContexts/terms.jsonld"))
        );
        pretty_assertions::assert_eq!(processed(&loaded), json!({ "@context": "terms.jsonld" }));
    }

    /// JSON-LD 1.1 API section 4.1.2 step 5.2.6: a download reached through a
    /// redirect hands the processor its relative references resolved against
    /// the final URL, while the durable row keeps the document as downloaded.
    #[tokio::test]
    async fn a_redirected_download_is_processed_against_its_final_url() {
        let origin = TestOrigin::start().await;
        origin.stage("/moved.jsonld", redirect("/contexts/v2/wrapper.jsonld")).await;
        origin
            .stage(
                "/contexts/v2/wrapper.jsonld",
                ok("application/ld+json", r#"{"@context": ["terms.jsonld", {"@import": "../shared.jsonld"}]}"#, &[]),
            )
            .await;

        let loaded = fetch_and_persist(&Client::new(), &origin.url("/moved.jsonld"), 2_000_000, CacheLifetime::DEFAULT, None)
            .await
            .expect("the redirect is followed");

        pretty_assertions::assert_eq!(
            processed(&loaded),
            json!({ "@context": [
                origin.url("/contexts/v2/terms.jsonld").as_str(),
                { "@import": origin.url("/contexts/shared.jsonld").as_str() }
            ] })
        );
        pretty_assertions::assert_eq!(loaded.document_url(&origin.url("/moved.jsonld")), &origin.url("/contexts/v2/wrapper.jsonld"));
    }
}
