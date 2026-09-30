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
    context_cache::ContextCache,
    core_context_precedence::without_embedded_core_context,
    engine::options::RemoteContextFetch,
    loader::{
        cache_lifetime::{CacheLifetime, ContextFreshness},
        error::{ContextUnavailable, LoaderError, LoaderErrorKind},
        loaded_context::{ContextDurability, EntryLifetime, LoadedContext},
        pending_persistence::PendingPersistence,
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
/// the origin server said lives thirty seconds.
///
/// # Errors
///
/// Returns [`ContextUnavailable::Cache`] when the durable read fails and remote
/// fetch is denied.
pub(crate) async fn load_from_durable_cache(
    cache: &dyn ContextCache,
    url: &IriBuf,
    allow_remote: RemoteContextFetch,
) -> Result<Option<LoadedContext>, LoaderError> {
    match cache.get(url).await {
        Ok(Some(hit)) => {
            trace!("L2 cache hit for context: {url}");
            let parsed = jsonld_syntax::Value::from_serde_json(hit.payload);
            Ok(Some(LoadedContext {
                value: Arc::new(without_embedded_core_context(url, parsed)),
                durability: ContextDurability::Durable,
                lifetime: EntryLifetime::of_residual(hit.residual),
                canonical_url: hit.canonical_url,
            }))
        }
        Ok(None) => Ok(None),
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

/// The remote step: fetch the context and, on a first success, persist it.
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
    let normalized = without_embedded_core_context(url, fetched.value);
    let lifetime = EntryLifetime::of(fetched.freshness);
    let ContextFreshness::Fresh(granted) = fetched.freshness else {
        // Clause 6.3.16: `no-store`, `no-cache` and an already-lapsed expiry all
        // leave nothing to invalidate later, so the document is served once and
        // never stored in either tier.
        return Ok(LoadedContext {
            value: Arc::new(normalized),
            durability: ContextDurability::Registry,
            lifetime,
            canonical_url: None,
        });
    };
    // Written to L2 only here, on the first remote-fetch success: the row
    // carries no counter, because hit accounting is the HitCounter's job.
    let durability = match l2_cache {
        Some(cache) => persist_to_durable_cache(cache.as_ref(), url, &normalized, granted).await,
        None => ContextDurability::Registry,
    };
    Ok(LoadedContext {
        value: Arc::new(normalized),
        durability,
        lifetime,
        canonical_url: None,
    })
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
/// lapses the resolver re-hits the registry and refreshes the row.
pub(crate) async fn persist_registry_context(
    cache: &dyn ContextCache,
    url: &IriBuf,
    value: Arc<jsonld_syntax::Value>,
    lifetime: CacheLifetime,
) -> LoadedContext {
    let durability = persist_to_durable_cache(cache, url, value.as_ref(), lifetime).await;
    LoadedContext {
        value,
        durability,
        lifetime: EntryLifetime::Bounded(lifetime),
        canonical_url: None,
    }
}

/// Persist a freshly fetched context, classifying the entry by whether the
/// durable write landed.
///
/// A write failure keeps the document serving from the in-process cache and
/// hands the payload to [`PendingPersistence`], so the next access retries it
/// rather than losing the row.
async fn persist_to_durable_cache(cache: &dyn ContextCache, url: &IriBuf, normalized: &jsonld_syntax::Value, lifetime: CacheLifetime) -> ContextDurability {
    let payload = jsonld_syntax::Value::into_serde_json(normalized.clone()); // clone: the normalized document is also returned to the caller
    match cache.put(url, &payload, lifetime).await {
        Ok(()) => {
            trace!("Persisted context to L2 cache: {url}");
            ContextDurability::Durable
        }
        Err(unwritten) => {
            warn!(url = %url, error = %unwritten, "Serving remotely loaded context while durable persistence is pending");
            ContextDurability::Pending(Arc::new(PendingPersistence::owed(payload, lifetime)))
        }
    }
}
