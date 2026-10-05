//! The position of one context source change in the order the changes of a
//! tenant committed.

use derive_more::{Display, From, Into};

/// The revision a write of a context source registration, or a client write of
/// a context source registration subscription, drew from its tenant's change
/// clock.
///
/// Revisions grow in the order the writes committed, whichever broker made
/// them, so of two changes the one with the lower revision committed first.
/// Clauses 5.11.2.4 and 5.11.3.4 notify a subscription of a registration
/// change against the subscription as that change found it, which is what
/// comparing the two revisions decides.
#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, From, Into)]
pub struct CsourceChangeRevision(i64);

#[cfg(test)]
mod tests {
    use crate::csource_change_revision::CsourceChangeRevision;

    #[test]
    fn a_revision_drawn_later_orders_after_one_drawn_earlier() {
        assert!(CsourceChangeRevision::from(4) < CsourceChangeRevision::from(5));
    }

    #[test]
    fn a_revision_reads_back_as_the_counter_value_it_was_drawn_as() {
        assert_eq!(i64::from(CsourceChangeRevision::from(17)), 17);
    }
}
