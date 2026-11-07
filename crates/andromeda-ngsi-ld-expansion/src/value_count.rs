//! Counting the values a write expands to against the entity value limit,
//! while the walk expands them.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.2 answers an input "too large" with 413
//! `RequestEntityTooLarge`, and the payload budget is counted on the written
//! document. A Property value of millions of numbers costs its expansion long
//! before that document exists, so the walk counts each value it expands under
//! a Property `value` or a `ListProperty` `valueList` and stops the moment the
//! count passes the limit.
//!
//! Each counted item is one value of the document census: a value object, a
//! JSON literal, a node or a list, wherever it stands below the carrier. The
//! census counts every one of them and more besides (the entity, its
//! instances, their members), so the walk stops only for a document the
//! census refuses, and the census still decides every other one.
//!
//! A member whose expansion does not map one to one onto the census (`object`,
//! `objectList`, `vocab`, `languageMap`, the entity `scope`) is counted before
//! it is expanded, by the census's own rule for it: the walk counts the member
//! as the census does and never the items its expansion makes.

use crate::decline::Decline;
use andromeda_ngsi_ld_number::json_value::value_census::ValueTally;

/// Where a part of a request stands for the value count.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ValueCount<'t> {
    /// The count the document's values go into; `None` when the walk counts
    /// nothing.
    tally: Option<&'t ValueTally>,
    /// Whether the part is a value the count reaches.
    position: Position,
}

/// Whether a part of a request is a value the count reaches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Position {
    /// The entity, an instance or one of its members other than a value.
    Structure,
    /// A Property value or a `valueList`, and everything below it.
    Value,
}

impl<'t> ValueCount<'t> {
    /// A walk that counts nothing.
    pub(crate) const fn uncounted() -> ValueCount<'t> {
        ValueCount {
            tally: None,
            position: Position::Structure,
        }
    }

    /// A walk whose values go into `tally`, starting at the entity.
    pub(crate) const fn into_tally(tally: Option<&'t ValueTally>) -> ValueCount<'t> {
        ValueCount {
            tally,
            position: Position::Structure,
        }
    }

    /// The same count, below a value carrier.
    pub(crate) const fn in_value(self) -> ValueCount<'t> {
        ValueCount {
            tally: self.tally,
            position: Position::Value,
        }
    }

    /// The room to make for the items of an array of `written` elements: as
    /// many as the count still admits and one more, the one that passes the
    /// limit, so an array far over the limit reserves no memory for what the
    /// walk never reaches.
    pub(crate) fn capacity(self, written: usize) -> usize {
        match (self.position, self.tally) {
            (Position::Value, Some(tally)) => written.min(tally.limit().get().saturating_sub(tally.counted().get()).saturating_add(1)),
            (Position::Structure, _) | (Position::Value, None) => written,
        }
    }

    /// Count one expanded value, when the part is a value the count reaches.
    ///
    /// # Errors
    ///
    /// Returns [`Decline::ValueLimit`] once the count passes the limit.
    pub(crate) fn count_one(self) -> Result<(), Decline> {
        match (self.position, self.tally) {
            (Position::Value, Some(tally)) => tally.admit_one().map_err(|_exceeded| Decline::ValueLimit),
            (Position::Structure, _) | (Position::Value, None) => Ok(()),
        }
    }

    /// Count `values` values a member holds by the census rule for that
    /// member, wherever the member stands. The member is counted before it is
    /// expanded, so one far over the limit is refused without being held.
    ///
    /// # Errors
    ///
    /// Returns [`Decline::ValueLimit`] once the count passes the limit.
    pub(crate) fn count_member(self, values: usize) -> Result<(), Decline> {
        self.tally.map_or(Ok(()), |tally| tally.admit(values).map_err(|_exceeded| Decline::ValueLimit))
    }
}

#[cfg(test)]
mod tests {
    use crate::{decline::Decline, value_count::ValueCount};
    use andromeda_ngsi_ld_number::json_value::value_census::{JsonValueLimit, ValueTally};
    use std::num::NonZeroUsize;

    fn tally(limit: usize) -> ValueTally {
        ValueTally::new(JsonValueLimit::new(NonZeroUsize::new(limit).expect("a positive limit")))
    }

    #[test]
    fn a_value_counts_until_the_count_passes_the_limit() {
        let tally = tally(2);
        let count = ValueCount::into_tally(Some(&tally)).in_value();
        pretty_assertions::assert_eq!(count.count_one(), Ok(()));
        pretty_assertions::assert_eq!(count.count_one(), Ok(()));
        pretty_assertions::assert_eq!(count.count_one(), Err(Decline::ValueLimit));
    }

    #[test]
    fn an_array_reserves_room_only_for_what_the_count_still_admits() {
        let tally = tally(3);
        let count = ValueCount::into_tally(Some(&tally)).in_value();
        count.count_one().expect("one value within three");
        pretty_assertions::assert_eq!(count.capacity(1_000), 3);
        pretty_assertions::assert_eq!(count.capacity(2), 2);
        pretty_assertions::assert_eq!(ValueCount::into_tally(Some(&tally)).capacity(1_000), 1_000);
    }

    #[test]
    fn the_structure_around_a_value_is_not_counted() {
        let tally = tally(1);
        let count = ValueCount::into_tally(Some(&tally));
        for _ in 0..3 {
            pretty_assertions::assert_eq!(count.count_one(), Ok(()));
        }
        pretty_assertions::assert_eq!(tally.counted().get(), 0);
    }

    #[test]
    fn a_member_counts_wherever_it_stands_until_the_count_passes_the_limit() {
        let tally = tally(5);
        let count = ValueCount::into_tally(Some(&tally));
        pretty_assertions::assert_eq!(count.count_member(5), Ok(()));
        pretty_assertions::assert_eq!(tally.counted().get(), 5);
        pretty_assertions::assert_eq!(count.count_member(1), Err(Decline::ValueLimit));
    }

    #[test]
    fn a_member_far_over_the_limit_is_refused_in_one_count() {
        let tally = tally(5);
        pretty_assertions::assert_eq!(ValueCount::into_tally(Some(&tally)).count_member(4_000_000), Err(Decline::ValueLimit));
    }

    #[test]
    fn an_uncounted_walk_counts_no_member() {
        pretty_assertions::assert_eq!(ValueCount::uncounted().count_member(usize::MAX), Ok(()));
    }

    #[test]
    fn an_uncounted_walk_counts_no_value() {
        let count = ValueCount::uncounted().in_value();
        for _ in 0..3 {
            pretty_assertions::assert_eq!(count.count_one(), Ok(()));
        }
    }
}
