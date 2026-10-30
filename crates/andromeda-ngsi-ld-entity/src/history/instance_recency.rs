//! Which of two instances of one attribute is the most current (clause
//! 4.5.5.3).
//!
//! Clause 4.5.5.3 settles a conflict between two instances of one attribute
//! with the same `datasetId`: instances past their `expiresAt` are discarded,
//! "thereafter the one with the most recent observedAt `DateTime`, if present,
//! and otherwise the one with the most recent modifiedAt `DateTime`", and two
//! instances neither tells apart leave the result indeterminate. "If present"
//! is read per instance: an instance carrying `observedAt` is the one with the
//! most recent observation over one carrying none, so `modifiedAt` decides
//! only between two instances that carry no `observedAt` or the same one. A
//! forwarded answer carries `modifiedAt` only when the client asked for
//! system attributes, so reading "if present" as present on both would leave
//! an observed instance and an unobserved one undecided. Every place
//! that decides which instance stands (merging the answers of several Context
//! Sources, folding a Temporal Evolution into current state) reads the same
//! order, and each decides for itself what an undecided tie keeps.

use chrono::{DateTime, Utc};
use std::cmp::Ordering;

/// The two instants clause 4.5.5.3 orders an instance by.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InstanceRecency {
    /// The instance's `observedAt`, when it carries one.
    pub observed_at: Option<DateTime<Utc>>,
    /// The instance's `modifiedAt`, when it carries one.
    pub modified_at: Option<DateTime<Utc>>,
}

impl InstanceRecency {
    /// How `self` orders against `other` under clause 4.5.5.3: the greater is
    /// the more current.
    ///
    /// Two distinct `observedAt` instants decide, and so does an `observedAt`
    /// on one side alone, which makes that side the more current; otherwise
    /// two `modifiedAt` instants do; an instance pair neither pair of instants
    /// tells apart is [`Ordering::Equal`], and the caller's own policy keeps
    /// one of them.
    #[must_use]
    pub fn order(&self, other: &InstanceRecency) -> Ordering {
        match (self.observed_at, other.observed_at) {
            (Some(own), Some(theirs)) if own != theirs => return own.cmp(&theirs),
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), Some(_)) | (None, None) => {}
        }
        match (self.modified_at, other.modified_at) {
            (Some(own), Some(theirs)) => own.cmp(&theirs),
            (None, Some(_) | None) | (Some(_), None) => Ordering::Equal,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::history::instance_recency::InstanceRecency;
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use std::cmp::Ordering;

    fn at(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("a valid instant").get()
    }

    fn recency(observed_at: Option<DateTime<Utc>>, modified_at: Option<DateTime<Utc>>) -> InstanceRecency {
        InstanceRecency { observed_at, modified_at }
    }

    /// Clause 4.5.5.3: the most recent `observedAt` is the most current.
    #[test]
    fn distinct_observed_instants_decide() {
        let older = recency(Some(at("2020-01-01T00:00:00Z")), Some(at("2030-01-01T00:00:00Z")));
        let newer = recency(Some(at("2021-01-01T00:00:00Z")), Some(at("2000-01-01T00:00:00Z")));
        assert_eq!(older.order(&newer), Ordering::Less);
        assert_eq!(newer.order(&older), Ordering::Greater);
    }

    /// "and otherwise the one with the most recent modifiedAt".
    #[test]
    fn modified_instants_decide_when_the_observed_ones_do_not() {
        let same = Some(at("2020-01-01T00:00:00Z"));
        assert_eq!(
            recency(same, Some(at("2020-01-01T00:00:00Z"))).order(&recency(same, Some(at("2020-01-02T00:00:00Z")))),
            Ordering::Less
        );
        assert_eq!(
            recency(None, Some(at("2020-01-02T00:00:00Z"))).order(&recency(None, Some(at("2020-01-01T00:00:00Z")))),
            Ordering::Greater
        );
    }

    /// "the one with the most recent observedAt `DateTime`, if present": only
    /// the observed instance carries one, so it is the most current whatever
    /// either `modifiedAt` says.
    #[test]
    fn an_observed_instance_is_more_current_than_an_unobserved_one() {
        let observed = recency(Some(at("2020-01-05T00:00:00Z")), Some(at("2020-01-01T00:00:00Z")));
        let unobserved = recency(None, Some(at("2020-01-02T00:00:00Z")));
        assert_eq!(observed.order(&unobserved), Ordering::Greater);
        assert_eq!(unobserved.order(&observed), Ordering::Less);
        assert_eq!(recency(None, None).order(&recency(Some(at("2020-01-01T00:00:00Z")), None)), Ordering::Less);
    }

    /// Two instances neither pair of instants tells apart is indeterminate.
    #[test]
    fn an_undecided_pair_is_equal() {
        assert_eq!(recency(None, None).order(&recency(None, Some(at("2020-01-01T00:00:00Z")))), Ordering::Equal);
        assert_eq!(recency(None, None).order(&recency(None, None)), Ordering::Equal);
    }
}
