//! The identity of one durable side-effect job.

use crate::uuid_v7_clock;
use derive_more::Display;
use uuid::Uuid;

/// Identity of one row of the durable side-effect queue.
///
/// The broker mints the value instead of letting the database default it, so
/// the identity of a queued job is known to the enqueueing code before its
/// statement runs and stays the same across a retry of that statement.
///
/// It is a version-7 UUID so queue rows are stored in enqueue order, which
/// keeps the primary key append-mostly rather than scattering inserts across
/// its whole key space. The millisecond a version-7 value discloses is not
/// client-visible information: a queue identity never leaves the broker.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[display("{_0}")]
pub struct SideEffectJobId(Uuid);

impl SideEffectJobId {
    /// Mint the identity of a job that is about to be enqueued.
    #[must_use]
    pub fn mint() -> SideEffectJobId {
        SideEffectJobId(uuid_v7_clock::mint())
    }

    /// Reconstruct the identity of a job read back from the queue.
    #[must_use]
    pub const fn from_uuid(value: Uuid) -> SideEffectJobId {
        SideEffectJobId(value)
    }

    /// Return the persistence representation.
    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::side_effect_job_id::SideEffectJobId;
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;

    #[test]
    fn minted_identities_are_version_seven() {
        assert_eq!(SideEffectJobId::mint().as_uuid().get_version_num(), 7);
    }

    #[test]
    fn identities_minted_in_one_millisecond_keep_mint_order() {
        let minted: Vec<SideEffectJobId> = (0..1024).map(|_| SideEffectJobId::mint()).collect();
        let mut sorted = minted.clone(); // clone: the comparison needs both the mint order and the sorted order.
        sorted.sort_unstable();
        assert_eq!(minted, sorted);
    }

    #[test]
    fn identities_minted_in_one_millisecond_are_distinct() {
        let minted: HashSet<SideEffectJobId> = (0..1024).map(|_| SideEffectJobId::mint()).collect();
        assert_eq!(minted.len(), 1024);
    }

    #[test]
    fn a_restored_identity_equals_the_identity_it_was_read_from() {
        let minted = SideEffectJobId::mint();
        assert_eq!(SideEffectJobId::from_uuid(minted.as_uuid()), minted);
    }
}
