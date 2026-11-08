//! Whether a lease-guarded acknowledgement reached the row it targeted.

/// Whether an acknowledgement reached a row this worker still held the lease on.
///
/// Every durable lease-owning queue in the broker, notification delivery and
/// the durable side-effect queue alike, guards its acknowledgement statements
/// by the row and the worker that claimed it, and some also by the lease still
/// being live, so an update that reaches zero rows says the same thing
/// regardless of which queue asked: the row is no longer this worker's, and
/// the caller must not treat its own attempt as having landed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeaseAcknowledgement {
    /// The row was still leased by this worker and the update applied.
    Applied,
    /// The row was no longer this worker's, because another worker reclaimed it
    /// or the guard required a live lease and it had lapsed. The acknowledgement
    /// was discarded rather than overwriting that worker's progress.
    Lost,
}

impl LeaseAcknowledgement {
    /// Read the outcome of a guarded statement off the rows it reached.
    ///
    /// A lease guard names one row by its key, so the statement reaches that
    /// row or nothing: no row means the guard refused, any row means it held.
    #[must_use]
    pub const fn of_guarded_update(rows_affected: u64) -> LeaseAcknowledgement {
        if rows_affected == 0 {
            LeaseAcknowledgement::Lost
        } else {
            LeaseAcknowledgement::Applied
        }
    }
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

    /// A guarded statement that reaches no row found the row gone or another
    /// worker's, so this worker's attempt did not land.
    #[test]
    fn a_guarded_update_that_reaches_no_row_is_lost() {
        assert_eq!(LeaseAcknowledgement::of_guarded_update(0), LeaseAcknowledgement::Lost);
    }

    #[test]
    fn a_guarded_update_that_reaches_its_row_is_applied() {
        assert_eq!(LeaseAcknowledgement::of_guarded_update(1), LeaseAcknowledgement::Applied);
    }
}
