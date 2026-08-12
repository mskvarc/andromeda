//! The bound on how large one tenant's attribute-name dictionary may grow.
//!
//! `attribute_names` is append-only: every previously unseen expanded attribute
//! IRI a client writes mints a new row, and nothing ever removes one. Attribute
//! IRIs are open-world domain values — ETSI GS CIM 009 clause 4.5.2 places no
//! restriction on which IRIs an Entity may carry — so any client can inflate the
//! dictionary indefinitely, and every entry costs a heap tuple, a unique-index
//! entry, and a slot in each tenant's in-process cache.
//!
//! Widening the dictionary key to `INTEGER` removed the 32,767-entry ceiling
//! that used to cap that inflation by accident. This module replaces the
//! accident with a stated policy: a per-tenant bound, refused as a typed error
//! rather than as a key-space exhaustion, and an observable count of how many
//! names the process has actually created.

use serde::{Deserialize, Serialize};
use std::{
    num::NonZeroU32,
    sync::atomic::{AtomicU64, Ordering},
};

/// Upper bound on the number of distinct attribute IRIs one tenant's
/// `attribute_names` dictionary may hold.
///
/// Non-zero because a dictionary that may hold no names admits no writes at
/// all: an Entity with any attribute would be rejected, which is not a
/// configuration anyone means to express.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct DictionaryNameLimit(NonZeroU32);

impl DictionaryNameLimit {
    /// The bound a tenant is given when none is configured for it.
    ///
    /// Chosen to sit far above any modelled domain — a data space's worth of
    /// NGSI-LD models runs to hundreds of distinct attribute IRIs, not tens of
    /// thousands — so it bites only on inflation, never on legitimate use.
    pub const DEFAULT: DictionaryNameLimit = DictionaryNameLimit(match NonZeroU32::new(10_000) {
        Some(limit) => limit,
        // Unreachable for a non-zero literal, and stated rather than unwrapped
        // because `Option::unwrap` is not available in a const context.
        None => NonZeroU32::MIN,
    });

    /// Build a bound from a non-zero count.
    pub const fn new(names: NonZeroU32) -> DictionaryNameLimit {
        DictionaryNameLimit(names)
    }

    /// The bound as a count.
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

/// A write would have taken the attribute-name dictionary past its bound.
///
/// Carries all three quantities because the operator's next decision — raise
/// the bound or find the client minting the names — depends on how far past the
/// bound the request reached, not merely on that it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("attribute-name dictionary limit reached: {existing} names recorded, {requested} new names requested, limit {limit}")]
pub struct DictionaryGrowthExceeded {
    /// Names the dictionary already held when the request was checked.
    pub existing: u64,
    /// New names the request would have created.
    pub requested: u64,
    /// The configured bound.
    pub limit: u32,
}

/// The per-tenant dictionary growth policy, and the count of names created
/// under it.
///
/// The guard is advisory in the concurrent case: it is checked against a count
/// read outside the inserting statement, so two writers racing to mint names
/// can together cross the bound by the size of one batch. That is deliberate.
/// Making the bound exact would need the count and the insert under one lock on
/// the dictionary, serialising every first write of every attribute; the bound
/// exists to stop unbounded inflation, and overshooting it by one batch does
/// not defeat that.
#[derive(Debug)]
pub struct DictionaryGrowthGuard {
    /// The configured bound.
    limit: DictionaryNameLimit,
    /// Names this process has created in this tenant's dictionary.
    created: AtomicU64,
}

impl DictionaryGrowthGuard {
    /// A guard enforcing `limit`, having created nothing yet.
    pub const fn new(limit: DictionaryNameLimit) -> DictionaryGrowthGuard {
        DictionaryGrowthGuard {
            limit,
            created: AtomicU64::new(0),
        }
    }

    /// Decide whether `requested` new names may be added to a dictionary that
    /// already holds `existing` of them.
    ///
    /// A request is admitted or refused whole: partially minting a batch would
    /// leave the write path holding names for some of an Entity's attributes
    /// and not others, which no caller can act on.
    pub fn admit(&self, existing: u64, requested: usize) -> Result<(), DictionaryGrowthExceeded> {
        // A `usize` wider than the bound can only be refused, so saturating at
        // `u64::MAX` fails closed rather than wrapping into acceptance.
        let requested = u64::try_from(requested).unwrap_or(u64::MAX);
        if requested == 0 {
            // A request that mints nothing cannot grow the dictionary, so the
            // bound does not apply to it -- including on a dictionary that is
            // already past the bound, which must still serve reads and writes
            // of the names it already holds.
            return Ok(());
        }
        if existing.saturating_add(requested) > u64::from(self.limit.get()) {
            return Err(DictionaryGrowthExceeded {
                existing,
                requested,
                limit: self.limit.get(),
            });
        }
        Ok(())
    }

    /// Record that `count` names were actually created.
    ///
    /// Counts creations rather than resolutions: a resolution that found an
    /// existing row cost the dictionary nothing, and conflating the two would
    /// make the figure track query volume instead of growth.
    pub fn record_created(&self, count: u64) {
        self.created.fetch_add(count, Ordering::Relaxed);
    }

    /// Names this process has created in this tenant's dictionary.
    pub fn created(&self) -> u64 {
        self.created.load(Ordering::Relaxed)
    }

    /// The bound being enforced.
    pub const fn limit(&self) -> DictionaryNameLimit {
        self.limit
    }
}

#[cfg(test)]
mod tests {
    use crate::dictionary_growth::{DictionaryGrowthGuard, DictionaryNameLimit};
    use std::num::NonZeroU32;

    fn limit(names: u32) -> DictionaryNameLimit {
        DictionaryNameLimit::new(NonZeroU32::new(names).expect("a non-zero test bound"))
    }

    #[test]
    fn a_batch_that_lands_exactly_on_the_bound_is_admitted() {
        let guard = DictionaryGrowthGuard::new(limit(10));
        assert!(guard.admit(7, 3).is_ok());
    }

    #[test]
    fn a_batch_that_would_cross_the_bound_is_refused_whole() {
        let guard = DictionaryGrowthGuard::new(limit(10));
        let refused = guard.admit(7, 4).expect_err("the batch crosses the bound");
        assert_eq!(refused.existing, 7);
        assert_eq!(refused.requested, 4);
        assert_eq!(refused.limit, 10);
    }

    /// A dictionary already past its bound — the bound was lowered, or two
    /// writers raced — refuses every further name rather than reporting the
    /// overshoot as headroom.
    #[test]
    fn a_dictionary_already_past_its_bound_admits_nothing_further() {
        let guard = DictionaryGrowthGuard::new(limit(10));
        assert!(guard.admit(11, 1).is_err());
    }

    /// A request resolving only names that already exist creates nothing, so it
    /// is admitted whatever the dictionary's size.
    #[test]
    fn a_request_creating_no_names_is_admitted_at_any_size() {
        let guard = DictionaryGrowthGuard::new(limit(10));
        assert!(guard.admit(10_000, 0).is_ok());
    }

    /// The counted quantity is names created, not names resolved: the guard is
    /// how an operator sees growth, and growth is only what was minted.
    #[test]
    fn the_observed_count_accumulates_only_created_names() {
        let guard = DictionaryGrowthGuard::new(limit(10));
        assert_eq!(guard.created(), 0);
        guard.record_created(2);
        guard.record_created(3);
        assert_eq!(guard.created(), 5);
    }

    #[test]
    fn the_default_bound_sits_far_above_any_modelled_domain() {
        assert_eq!(DictionaryNameLimit::DEFAULT.get(), 10_000);
    }
}
