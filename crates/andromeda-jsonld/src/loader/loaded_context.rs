//! One resolved `@context` as the in-process cache holds it.
//!
//! An entry carries two things the cache cannot infer: where its durable row
//! and hit accounting live, and how long it may be kept. The second is what
//! clause 6.3.16 governs — the entry lives for the span the response granted,
//! not for a fixed configured time — so the in-process cache is driven by a
//! per-entry [`Expiry`] rather than by one cache-wide time-to-live.

use crate::loader::{
    cache_lifetime::{CacheLifetime, ContextFreshness, ResidualLifetime},
    pending_persistence::PendingPersistence,
};
use iri_rs::IriBuf;
use moka::Expiry;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// A loaded context together with the backing-store ownership of its hit count.
///
/// Only contexts that have a durable cache row participate in clause 5.13.3.5
/// accounting. Registry and preloaded contexts have no row to update.
#[derive(Debug, Clone)]
pub(crate) struct LoadedContext {
    /// The parsed JSON-LD context payload supplied to the processor.
    pub(crate) value: Arc<jsonld_syntax::Value>,
    /// Where the cached document's durability and hit accounting live.
    pub(crate) durability: ContextDurability,
    /// How long the in-process cache may keep this entry.
    pub(crate) lifetime: EntryLifetime,
}

/// Durability and hit-accounting ownership of one in-process entry.
#[derive(Debug, Clone)]
pub(crate) enum ContextDurability {
    /// Embedded or preloaded registry entry with no durable hit row.
    Registry,
    /// Entry already persisted in the durable cache.
    Durable,
    /// Remotely loaded entry awaiting persistence.
    Pending(Arc<PendingPersistence>),
}

/// How long the in-process cache may keep one entry.
///
/// Distinct from [`ContextFreshness`], which describes only what a *response*
/// permitted: an embedded or preloaded document has no response at all, and an
/// entry read back from the durable cache is bounded by what is left of the
/// span the original response granted rather than by that whole span again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryLifetime {
    /// The entry is dropped at once: the response forbade storing it.
    Immediate,
    /// The entry is dropped once this lifetime has elapsed.
    Bounded(CacheLifetime),
    /// The entry never expires: an embedded or preloaded document has no origin
    /// server to grow stale against.
    Unbounded,
}

impl EntryLifetime {
    /// How long a response's freshness lets the in-process cache keep an entry.
    pub(crate) const fn of(freshness: ContextFreshness) -> EntryLifetime {
        match freshness {
            ContextFreshness::Uncacheable => EntryLifetime::Immediate,
            ContextFreshness::Fresh(lifetime) => EntryLifetime::Bounded(lifetime),
        }
    }

    /// How long a durable-cache hit lets the in-process cache keep an entry.
    ///
    /// Clause 6.3.16 binds every tier to the span the origin server granted, so
    /// a hit is held to what is left of that span and lapses in memory at the
    /// instant the stored row does.
    pub(crate) const fn of_residual(residual: ResidualLifetime) -> EntryLifetime {
        match residual {
            ResidualLifetime::Unbounded => EntryLifetime::Unbounded,
            ResidualLifetime::Bounded(lifetime) => EntryLifetime::Bounded(lifetime),
        }
    }
}

/// A cacheable entry that owns no durable hit row.
///
/// Registry documents are embedded or preloaded, so clause 5.13.3.5 accounting
/// has no `jsonld_contexts` row to attribute an access to, and no origin server
/// can invalidate them.
pub(crate) const fn registry_context(value: Arc<jsonld_syntax::Value>) -> LoadedContext {
    LoadedContext {
        value,
        durability: ContextDurability::Registry,
        lifetime: EntryLifetime::Unbounded,
    }
}

/// Gives every in-process entry the lifetime clause 6.3.16 derived for it.
///
/// Returning `None` clears the expiry, which is why an entry the response
/// forbade storing reports a zero duration instead: it must lapse, not persist.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ResponseDrivenExpiry;

impl Expiry<IriBuf, LoadedContext> for ResponseDrivenExpiry {
    fn expire_after_create(&self, _url: &IriBuf, loaded: &LoadedContext, _created_at: Instant) -> Option<Duration> {
        match loaded.lifetime {
            EntryLifetime::Immediate => Some(Duration::ZERO),
            EntryLifetime::Bounded(lifetime) => Some(lifetime.as_duration()),
            EntryLifetime::Unbounded => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::loader::{
        cache_lifetime::{CacheLifetime, ContextFreshness, ResidualLifetime},
        loaded_context::{ContextDurability, EntryLifetime, LoadedContext, ResponseDrivenExpiry},
    };
    use iri_rs::IriBuf;
    use moka::Expiry;
    use serde_json::json;
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };

    fn expiry_of(lifetime: EntryLifetime) -> Option<Duration> {
        let url = IriBuf::new("https://example.com/context.jsonld".to_string()).expect("valid URL");
        let loaded = LoadedContext {
            value: Arc::new(jsonld_syntax::Value::from_serde_json(json!({ "@context": {} }))),
            durability: ContextDurability::Registry,
            lifetime,
        };
        ResponseDrivenExpiry.expire_after_create(&url, &loaded, Instant::now())
    }

    /// Clause 6.3.16: the entry lives for the span the response granted, so a
    /// `max-age` of sixty seconds is sixty seconds and not the configured TTL.
    #[test]
    fn a_granted_lifetime_becomes_the_in_process_expiry() {
        let lifetime = CacheLifetime::from_seconds(60).expect("a minute is a positive lifetime");

        pretty_assertions::assert_eq!(expiry_of(EntryLifetime::of(ContextFreshness::Fresh(lifetime))), Some(Duration::from_secs(60)));
    }

    #[test]
    fn a_document_the_response_forbade_storing_lapses_at_once() {
        pretty_assertions::assert_eq!(expiry_of(EntryLifetime::of(ContextFreshness::Uncacheable)), Some(Duration::ZERO));
    }

    #[test]
    fn an_embedded_document_never_expires() {
        pretty_assertions::assert_eq!(expiry_of(EntryLifetime::Unbounded), None);
    }

    /// Clause 6.3.16: a durable hit is kept only for what is left of the span
    /// the origin server granted, so the in-process copy lapses when the stored
    /// row does rather than outliving it by the configured heuristic.
    #[test]
    fn a_durable_hit_keeps_only_the_lifetime_the_row_has_left() {
        let remaining = CacheLifetime::from_seconds(12).expect("twelve seconds is a positive lifetime");

        pretty_assertions::assert_eq!(
            expiry_of(EntryLifetime::of_residual(ResidualLifetime::Bounded(remaining))),
            Some(Duration::from_secs(12))
        );
        pretty_assertions::assert_eq!(expiry_of(EntryLifetime::of_residual(ResidualLifetime::Unbounded)), None);
    }
}
