//! The bound on how large one tenant's name dictionaries may grow.
//!
//! `attribute_names` and `entity_type_names` are append-only: every previously
//! unseen expanded attribute or type IRI a client writes mints a new row, and
//! nothing ever removes one. Both are open-world domain values — ETSI GS CIM
//! 009 clauses 4.5.1 and 4.5.2 place no restriction on which IRIs an Entity may
//! carry — so any client can inflate a dictionary indefinitely, and every entry
//! costs a heap tuple, a unique-index entry, and a slot in each tenant's
//! in-process cache.
//!
//! Widening the dictionary key to `INTEGER` removed the 32,767-entry ceiling
//! that used to cap that inflation by accident. This module replaces the
//! accident with a stated policy: a per-tenant bound, refused as a typed error
//! rather than as a key-space exhaustion.
//!
//! The bound is decided against the dictionary's own size, read inside the
//! transaction that is about to mint names. Nothing in this process tallies how
//! much of the bound has been taken: names are minted inside a caller's
//! transaction and a rollback takes them away again, so a process-side tally
//! would count creations the database never kept, and would refuse later writes
//! on the strength of writes that never happened.

use crate::dictionary_relation::DictionaryKind;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

/// Upper bound on the number of distinct IRIs one of a tenant's name
/// dictionaries may hold.
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
    /// NGSI-LD models runs to hundreds of distinct attribute or type IRIs, not
    /// tens of thousands — so it bites only on inflation, never on legitimate
    /// use.
    pub const DEFAULT: DictionaryNameLimit = DictionaryNameLimit(match NonZeroU32::new(10_000) {
        Some(limit) => limit,
        // Unreachable for a non-zero literal, and stated rather than unwrapped
        // because `Option::unwrap` is not available in a const context.
        None => NonZeroU32::MIN,
    });

    /// Build a bound from a non-zero count.
    #[must_use]
    pub const fn new(names: NonZeroU32) -> DictionaryNameLimit {
        DictionaryNameLimit(names)
    }

    /// The bound as a count.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }

    /// Decide whether `requested` new names may be added to the `dictionary`
    /// that already holds `existing` of them.
    ///
    /// A request is admitted or refused whole: partially minting a batch would
    /// leave the write path holding names for some of an Entity's attributes
    /// and not others, which no caller can act on.
    ///
    /// `existing` is the size the caller's own transaction sees, so it already
    /// includes the names earlier phases of that same transaction minted, and
    /// it excludes the names of a transaction that rolled back.
    ///
    /// # Errors
    ///
    /// Returns [`DictionaryGrowthExceeded`] when the batch would take the
    /// dictionary past the bound.
    pub fn admit(self, dictionary: DictionaryKind, existing: u64, requested: usize) -> Result<(), DictionaryGrowthExceeded> {
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
        if existing.saturating_add(requested) > u64::from(self.get()) {
            return Err(DictionaryGrowthExceeded {
                dictionary,
                existing,
                requested,
                limit: self.get(),
            });
        }
        Ok(())
    }
}

/// A write would have taken a name dictionary past its bound.
///
/// Carries all three quantities because the operator's next decision — raise
/// the bound or find the client minting the names — depends on how far past the
/// bound the request reached, not merely on that it did.
///
/// The bound is exact only against one writer at a time: two transactions each
/// read the dictionary's size before either commits, so together they can cross
/// it by the size of one batch. Making it exact would need the size and the
/// insert under one lock on the whole dictionary, serialising every first write
/// of every name; the bound exists to stop unbounded inflation, and
/// overshooting it by one batch does not defeat that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{dictionary} dictionary limit reached: {existing} names recorded, {requested} new names requested, limit {limit}")]
pub struct DictionaryGrowthExceeded {
    /// Which dictionary refused the batch.
    pub dictionary: DictionaryKind,
    /// Names the dictionary already held when the request was checked.
    pub existing: u64,
    /// New names the request would have created.
    pub requested: u64,
    /// The configured bound.
    pub limit: u32,
}

#[cfg(test)]
mod tests {
    use crate::{dictionary_growth::DictionaryNameLimit, dictionary_relation::DictionaryKind};
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU32;

    const KIND: DictionaryKind = DictionaryKind::AttributeNames;

    fn limit(names: u32) -> DictionaryNameLimit {
        DictionaryNameLimit::new(NonZeroU32::new(names).expect("a non-zero test bound"))
    }

    #[test]
    fn a_batch_that_lands_exactly_on_the_bound_is_admitted() {
        assert!(limit(10).admit(KIND, 7, 3).is_ok());
    }

    #[test]
    fn a_batch_that_would_cross_the_bound_is_refused_whole() {
        let refused = limit(10).admit(KIND, 7, 4).expect_err("the batch crosses the bound");
        assert_eq!(refused.dictionary, KIND);
        assert_eq!(refused.existing, 7);
        assert_eq!(refused.requested, 4);
        assert_eq!(refused.limit, 10);
    }

    /// A dictionary already past its bound — the bound was lowered, or two
    /// writers raced — refuses every further name rather than reporting the
    /// overshoot as headroom.
    #[test]
    fn a_dictionary_already_past_its_bound_admits_nothing_further() {
        assert!(limit(10).admit(KIND, 11, 1).is_err());
    }

    /// A request resolving only names that already exist creates nothing, so it
    /// is admitted whatever the dictionary's size.
    #[test]
    fn a_request_creating_no_names_is_admitted_at_any_size() {
        assert!(limit(10).admit(KIND, 10_000, 0).is_ok());
    }

    /// The budget a write takes is held by the dictionary itself, inside that
    /// write's transaction. A write that minted its whole batch and then rolled
    /// back leaves the size it read behind it, so the next write is offered the
    /// same headroom; nothing here remembers the batch that vanished.
    #[test]
    fn a_rolled_back_batch_leaves_its_budget_to_the_next_write() {
        let limit = limit(10);
        assert!(limit.admit(KIND, 8, 2).is_ok(), "the first write fills the dictionary to its bound");
        assert!(
            limit.admit(KIND, 8, 2).is_ok(),
            "the first write rolled back, so the next one sees the same size and the same headroom"
        );
        assert!(
            limit.admit(KIND, 10, 1).is_err(),
            "had the first write committed, the size it left refuses the next name"
        );
    }

    #[test]
    fn the_default_bound_sits_far_above_any_modelled_domain() {
        assert_eq!(DictionaryNameLimit::DEFAULT.get(), 10_000);
    }
}
