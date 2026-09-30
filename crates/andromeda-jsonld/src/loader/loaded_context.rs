//! One resolved `@context` as the in-process cache holds it.
//!
//! An entry carries two things the cache cannot infer: where its durable row
//! and hit accounting live, and how long it may be kept. The second is what
//! clause 6.3.16 governs — the entry lives for the span the response granted,
//! not for a fixed configured time — so the in-process cache is driven by a
//! per-entry [`Expiry`] rather than by one cache-wide time-to-live.

use crate::{
    context_registry::RegisteredContext,
    context_tail::ContextTail,
    loader::{
        cache_lifetime::{CacheLifetime, ContextFreshness, ResidualLifetime},
        pending_persistence::PendingPersistence,
    },
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
    /// What the document as written leaves in force last (clause 4.4), read
    /// before its embedded Core @context references were removed from `value`.
    pub(crate) tail: ContextTail,
    /// Where the cached document's durability and hit accounting live.
    pub(crate) durability: ContextDurability,
    /// How long the in-process cache may keep this entry.
    pub(crate) lifetime: EntryLifetime,
    /// The URL clause 5.13.3.5 hit accounting is attributed to, when it differs
    /// from the URL the request resolved the entry by.
    ///
    /// A durable row is reachable by either of its identities, so a hosted
    /// `@context` echoed back as a relative path resolves by its local id while
    /// its `numberOfHits` keys on the serving URL; this carries that serving URL
    /// so the access is counted under the same key the metadata is read under.
    /// `None` attributes the access to the resolved URL itself.
    pub(crate) canonical_url: Option<IriBuf>,
    /// Where the document came from, which decides the URL every relative
    /// reference inside it resolves against and whether it is one the broker
    /// stores itself.
    pub(crate) provenance: ContextProvenance,
}

/// Where a loaded document came from.
///
/// It decides two things. The URL relative references inside the document
/// resolve against (the JSON-LD 1.1 API's `RemoteDocument.documentUrl`, "the
/// final URL of the loaded document"). And whether the entry is one of the
/// broker's own clause 5.13.1 "Hosted" or "`ImplicitlyCreated`" documents,
/// which alone may be served under a stored identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ContextProvenance {
    /// An embedded or operator-preloaded document, based on the URL it was
    /// resolved by.
    Local,
    /// A download, based on the URL it was finally read from after redirects
    /// and a JSON-LD alternate, whether it was just downloaded or read back
    /// from the durable row that stores it (a clause 5.13.1 "Cached" entry).
    Downloaded(IriBuf),
    /// A document the broker stores itself (clause 5.13.1 "Hosted" or
    /// "`ImplicitlyCreated`"), which was never read from anywhere and is based
    /// on the URL it was resolved by.
    Broker,
}

impl ContextProvenance {
    /// The URL relative references inside the document resolve against, given
    /// the URL the entry was resolved by.
    pub(crate) const fn document_url<'a>(&'a self, resolved: &'a IriBuf) -> &'a IriBuf {
        match self {
            ContextProvenance::Local | ContextProvenance::Broker => resolved,
            ContextProvenance::Downloaded(url) => url,
        }
    }
}

impl LoadedContext {
    /// The URL relative references inside the document resolve against, given
    /// the URL the entry was resolved by.
    pub(crate) const fn document_url<'a>(&'a self, resolved: &'a IriBuf) -> &'a IriBuf {
        self.provenance.document_url(resolved)
    }

    /// What the document leaves in force last, once a relative reference in
    /// last place is resolved against the document's URL, given the URL the
    /// entry was resolved by.
    ///
    /// Resolved here rather than when the document was read because the URL a
    /// document the broker stores itself is processed under is the one the
    /// request named it by, which is not the identifier it is stored under.
    pub(crate) fn tail_from(&self, resolved: &IriBuf) -> ContextTail {
        self.tail.clone().resolved_against(self.document_url(resolved)) // clone: the entry keeps its tail for the next resolution
    }
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
    /// The entry never expires: an embedded document has no origin server to
    /// grow stale against and no row another process could change.
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
    ///
    /// A row with no expiry is a clause 5.13.1 `"Hosted"` or
    /// `"ImplicitlyCreated"` entry, which no origin server can make stale but
    /// which another broker process sharing the database can delete. Holding it
    /// for the life of this process would keep a deleted document resolvable
    /// here indefinitely, so its copy is bounded by `hosted_revalidation`, after
    /// which the next use reads the row again.
    pub(crate) const fn of_residual(residual: ResidualLifetime, hosted_revalidation: CacheLifetime) -> EntryLifetime {
        match residual {
            ResidualLifetime::Unbounded => EntryLifetime::Bounded(hosted_revalidation),
            ResidualLifetime::Bounded(lifetime) => EntryLifetime::Bounded(lifetime),
        }
    }
}

/// A cacheable entry that owns no durable hit row.
///
/// Registry documents are embedded or preloaded, so clause 5.13.3.5 accounting
/// has no `jsonld_contexts` row to attribute an access to, and no origin server
/// can invalidate them.
pub(crate) fn registry_context(registered: RegisteredContext) -> LoadedContext {
    LoadedContext {
        value: registered.value,
        tail: registered.tail,
        durability: ContextDurability::Registry,
        lifetime: EntryLifetime::Unbounded,
        canonical_url: None,
        provenance: ContextProvenance::Local,
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
    use crate::{
        context_tail::ContextTail,
        loader::{
            cache_lifetime::{CacheLifetime, ContextFreshness, ResidualLifetime},
            loaded_context::{ContextDurability, ContextProvenance, EntryLifetime, LoadedContext, ResponseDrivenExpiry},
        },
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
            tail: ContextTail::LocalDefinitions,
            durability: ContextDurability::Registry,
            lifetime,
            canonical_url: None,
            provenance: ContextProvenance::Local,
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

    /// A revalidation span distinct from every other lifetime in these tests.
    fn hosted_revalidation() -> CacheLifetime {
        CacheLifetime::from_seconds(45).expect("forty-five seconds is a positive lifetime")
    }

    /// Clause 6.3.16: a cached download's durable hit is kept only for what is
    /// left of the span the origin server granted, so the in-process copy
    /// lapses when the stored row does, whatever the revalidation span is.
    #[test]
    fn a_durable_hit_of_a_cached_download_keeps_only_the_lifetime_the_row_has_left() {
        let remaining = CacheLifetime::from_seconds(12).expect("twelve seconds is a positive lifetime");

        pretty_assertions::assert_eq!(
            expiry_of(EntryLifetime::of_residual(ResidualLifetime::Bounded(remaining), hosted_revalidation())),
            Some(Duration::from_secs(12))
        );
    }

    /// A row with no expiry is one the broker stores itself; another process
    /// sharing the database can delete it, so the in-process copy is read
    /// again after the configured revalidation span rather than never.
    #[test]
    fn a_durable_hit_of_a_broker_stored_context_is_revalidated_after_the_configured_span() {
        pretty_assertions::assert_eq!(
            expiry_of(EntryLifetime::of_residual(ResidualLifetime::Unbounded, hosted_revalidation())),
            Some(Duration::from_secs(45))
        );
    }

    /// A download reports where it was finally read from; every other entry
    /// the URL it was resolved by.
    #[test]
    fn the_document_url_is_the_final_url_of_a_download_and_the_resolved_url_otherwise() {
        let resolved = IriBuf::new("https://example.com/requested.jsonld".to_string()).expect("valid URL");
        let final_url = IriBuf::new("https://example.com/moved/context.jsonld".to_string()).expect("valid URL");
        let mut loaded = LoadedContext {
            value: Arc::new(jsonld_syntax::Value::from_serde_json(json!({ "@context": {} }))),
            tail: ContextTail::LocalDefinitions,
            durability: ContextDurability::Registry,
            lifetime: EntryLifetime::Unbounded,
            canonical_url: None,
            provenance: ContextProvenance::Local,
        };
        pretty_assertions::assert_eq!(loaded.document_url(&resolved), &resolved);

        loaded.provenance = ContextProvenance::Downloaded(final_url.clone()); // clone: the expectation compares against it
        pretty_assertions::assert_eq!(loaded.document_url(&resolved), &final_url);

        loaded.provenance = ContextProvenance::Broker;
        pretty_assertions::assert_eq!(loaded.document_url(&resolved), &resolved);
    }

    /// A relative reference the document ends with resolves against the URL
    /// the document is processed under: for a document the broker stores, the
    /// URL the request named, not the identifier it is stored under.
    #[test]
    fn a_relative_tail_resolves_against_the_url_the_entry_is_resolved_by() {
        let requested = IriBuf::new("https://broker.example.com/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:outer".to_string()).expect("valid URL");
        let loaded = LoadedContext {
            value: Arc::new(jsonld_syntax::Value::from_serde_json(json!({ "@context": "terms.jsonld" }))),
            tail: ContextTail::of_document(&json!({ "@context": "terms.jsonld" })),
            durability: ContextDurability::Durable,
            lifetime: EntryLifetime::Unbounded,
            canonical_url: None,
            provenance: ContextProvenance::Broker,
        };

        pretty_assertions::assert_eq!(
            loaded.tail_from(&requested),
            ContextTail::Reference(IriBuf::new("https://broker.example.com/ngsi-ld/v1/jsonldContexts/terms.jsonld".to_string()).expect("valid URL"))
        );
    }
}
