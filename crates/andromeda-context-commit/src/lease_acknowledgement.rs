//! Whether a lease-guarded acknowledgement reached the row it targeted.

/// Whether an acknowledgement reached a row this worker still held the lease on.
///
/// Every durable lease-owning queue in the broker — notification delivery and
/// the durable side-effect queue alike — guards its acknowledgement statements
/// by owner and live lease, so an update that reaches zero rows says the same
/// thing regardless of which queue asked: the lease expired and another worker
/// now owns the row, and the caller must not treat its own attempt as having
/// landed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeaseAcknowledgement {
    /// The row was still leased by this worker and the update applied.
    Applied,
    /// The lease had expired and another worker owns the row, so the
    /// acknowledgement was discarded rather than overwriting that worker's
    /// progress.
    Lost,
}

#[cfg(test)]
mod tests {
    use crate::lease_acknowledgement::LeaseAcknowledgement;
    use pretty_assertions::{assert_eq, assert_ne};

    #[test]
    fn applied_and_lost_are_distinct_outcomes() {
        assert_ne!(LeaseAcknowledgement::Applied, LeaseAcknowledgement::Lost);
    }

    #[test]
    fn the_outcome_is_copied_rather_than_moved_out_of_a_shared_reference() {
        let outcome = LeaseAcknowledgement::Applied;
        let reference = &outcome;
        let copied: LeaseAcknowledgement = *reference;
        assert_eq!(copied, outcome);
    }
}
