//! What a context keeps while its durable write is still owed.
//!
//! A remote fetch that resolves but whose durable write fails must not cost the
//! request its `@context`: the document is served from the in-process cache and
//! the write is retried on the next access. Everything that retry needs — the
//! payload, where it was read from, the lifetime clause 6.3.16 derived for it,
//! and the hits that accrued meanwhile — is held here until it lands.

use crate::loader::cache_lifetime::CacheLifetime;
use andromeda_json_literal::JsonLiteral;
use iri_rs::IriBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// State retained while an in-process context awaits durable persistence.
#[derive(Debug)]
pub(crate) struct PendingPersistence {
    /// Valid remote payload served while persistence is pending.
    pub(crate) payload: JsonLiteral,
    /// The URL the payload was finally read from, replayed on the retried
    /// write so the stored row resolves its references as the download did.
    pub(crate) document_url: IriBuf,
    /// The lifetime the response granted, replayed on the retried write so the
    /// stored row expires when clause 6.3.16 says it must.
    pub(crate) lifetime: CacheLifetime,
    /// Single-flight guard for the persistence retry.
    pub(crate) retrying: AtomicBool,
    /// In-process hits accumulated before persistence completes.
    pub(crate) hits: AtomicU64,
    /// Whether a retry has completed durable persistence.
    pub(crate) durable: AtomicBool,
}

impl PendingPersistence {
    /// The state a failed durable write leaves behind.
    pub(crate) const fn owed(payload: JsonLiteral, document_url: IriBuf, lifetime: CacheLifetime) -> PendingPersistence {
        PendingPersistence {
            payload,
            document_url,
            lifetime,
            retrying: AtomicBool::new(false),
            hits: AtomicU64::new(0),
            durable: AtomicBool::new(false),
        }
    }
}

/// Cancellation-safe ownership of one persistence retry.
///
/// The guard is released on drop, so a caller cancelled mid-retry does not
/// leave the single-flight flag raised and the retry unreachable forever.
pub(crate) struct PersistenceRetryLease<'a>(pub(crate) &'a AtomicBool);

impl Drop for PersistenceRetryLease<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use crate::loader::{
        cache_lifetime::CacheLifetime,
        pending_persistence::{PendingPersistence, PersistenceRetryLease},
    };
    use andromeda_json_literal::JsonLiteral;
    use iri_rs::IriBuf;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn a_dropped_retry_lease_releases_the_single_flight_guard() {
        let guard = AtomicBool::new(true);
        drop(PersistenceRetryLease(&guard));

        assert!(!guard.load(Ordering::Acquire), "the next access must be able to retry the write");
    }

    #[test]
    fn owed_persistence_starts_with_no_hits_and_no_durability() {
        let lifetime = CacheLifetime::from_seconds(30).expect("thirty seconds is a positive lifetime");
        let document_url = IriBuf::new("https://example.com/final.jsonld".to_owned()).expect("a valid IRI");
        let pending = PendingPersistence::owed(JsonLiteral::null(), document_url.clone(), lifetime); // clone: the expectation compares against it

        pretty_assertions::assert_eq!(pending.hits.load(Ordering::Relaxed), 0);
        assert!(!pending.durable.load(Ordering::Acquire));
        assert!(!pending.retrying.load(Ordering::Acquire));
        pretty_assertions::assert_eq!(pending.lifetime, lifetime);
        pretty_assertions::assert_eq!(pending.document_url, document_url);
    }
}
