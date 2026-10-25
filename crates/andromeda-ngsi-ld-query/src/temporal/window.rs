//! The interval of the time axis a query reads.

use crate::error::QueryError;
use andromeda_ngsi_ld_lexicon::{time_prop::TimeProp, time_rel::TimeRel};
use chrono::{DateTime, Utc};
use typed_builder::TypedBuilder;

/// The slice of the time axis a query is bounded to (clause 4.11).
///
/// `timerel` and `timeAt` are meaningless apart (clause 5.7.3 requires both or
/// neither), `endTimeAt` belongs to `between` alone, and a `between` interval
/// closes after it opens. Stating that in the type means SQL builders need no
/// runtime checks for it, and none has to raise a *request* error from inside a
/// statement builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeInterval {
    /// The query bounded nothing; every instant qualifies.
    Unbounded,
    /// Everything strictly before the instant.
    Before(DateTime<Utc>),
    /// Everything at or after the instant.
    After(DateTime<Utc>),
    /// Everything from `from` inclusive to `to` exclusive, `to` later than
    /// `from`.
    Between {
        /// The opening instant, inclusive.
        from: DateTime<Utc>,
        /// The closing instant, exclusive.
        to: DateTime<Utc>,
    },
}

impl TimeInterval {
    /// Build an interval from the three parameters a request carries.
    ///
    /// Clause 4.11 uses `endTimeAt` "only for the between relation", and every
    /// table carrying it gives it cardinality 1 with `between` and 0 otherwise.
    /// A `between` whose `endTimeAt` is not after its `timeAt` selects no instant
    /// (the lower bound is inclusive, the upper exclusive), so it is a malformed
    /// window rather than an empty one.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::ParamTimerelTimeatPair`] when exactly one of
    /// `timerel` and `timeAt` is present,
    /// [`QueryError::ParamEndTimeAtRequired`] when `timerel` is `between` and
    /// no `endTimeAt` came with it, [`QueryError::ParamEndTimeAtWithoutBetween`]
    /// when an `endTimeAt` came with any other `timerel` or with none, and
    /// [`QueryError::ParamEndTimeAtNotAfterTimeAt`] when a `between` closes at
    /// or before the instant it opens at.
    pub fn from_parts(timerel: Option<TimeRel>, time_at: Option<DateTime<Utc>>, end_time_at: Option<DateTime<Utc>>) -> Result<TimeInterval, QueryError> {
        match (timerel, time_at, end_time_at) {
            (None, None, None) => Ok(TimeInterval::Unbounded),
            (None, Some(_), _) | (Some(_), None, _) => Err(QueryError::ParamTimerelTimeatPair),
            (Some(TimeRel::Before), Some(instant), None) => Ok(TimeInterval::Before(instant)),
            (Some(TimeRel::After), Some(instant), None) => Ok(TimeInterval::After(instant)),
            (Some(TimeRel::Between), Some(from), Some(to)) if to > from => Ok(TimeInterval::Between { from, to }),
            (Some(TimeRel::Between), Some(_), Some(_)) => Err(QueryError::ParamEndTimeAtNotAfterTimeAt),
            (Some(TimeRel::Between), Some(_), None) => Err(QueryError::ParamEndTimeAtRequired),
            (Some(TimeRel::Before | TimeRel::After), Some(_), Some(_)) | (None, None, Some(_)) => Err(QueryError::ParamEndTimeAtWithoutBetween),
        }
    }

    /// The reference instant, which every bounded interval carries.
    #[must_use]
    pub const fn reference(&self) -> Option<&DateTime<Utc>> {
        match self {
            TimeInterval::Unbounded => None,
            TimeInterval::Before(instant) | TimeInterval::After(instant) | TimeInterval::Between { from: instant, .. } => Some(instant),
        }
    }

    /// The instant the interval opens at, which `after` and `between` carry.
    /// `None` when the interval reaches back without limit.
    #[must_use]
    pub const fn opening(&self) -> Option<&DateTime<Utc>> {
        match self {
            TimeInterval::Unbounded | TimeInterval::Before(_) => None,
            TimeInterval::After(from) | TimeInterval::Between { from, .. } => Some(from),
        }
    }

    /// The instant the interval closes at, which `before` and `between` carry.
    /// `None` when the interval reaches forward without limit.
    #[must_use]
    pub const fn closing(&self) -> Option<&DateTime<Utc>> {
        match self {
            TimeInterval::Unbounded | TimeInterval::After(_) => None,
            TimeInterval::Before(to) | TimeInterval::Between { to, .. } => Some(to),
        }
    }

    /// Whether the interval narrows the time axis at all.
    #[must_use]
    pub const fn is_bounded(&self) -> bool {
        !matches!(self, TimeInterval::Unbounded)
    }

    /// Whether `instant` lies in the interval, with the bounds of clause 4.11:
    /// `before` exclusive, `after` inclusive, `between` inclusive at its
    /// opening and exclusive at its closing instant.
    #[must_use]
    pub fn holds(&self, instant: DateTime<Utc>) -> bool {
        match self {
            TimeInterval::Unbounded => true,
            TimeInterval::Before(to) => instant < *to,
            TimeInterval::After(from) => instant >= *from,
            TimeInterval::Between { from, to } => instant >= *from && instant < *to,
        }
    }

    /// Whether the period from `start` to `end` lies wholly in the interval.
    /// A period's `end` closes it the way the interval's closing instant
    /// closes the interval (clause 4.5.19), so a period ending on that
    /// instant still lies in it; a side the period leaves open lies in it.
    #[must_use]
    pub fn holds_period(&self, start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) -> bool {
        let opens_inside = match (self.opening(), start) {
            (Some(opening), Some(start)) => start >= *opening,
            (None, _) | (_, None) => true,
        };
        let closes_inside = match (self.closing(), end) {
            (Some(closing), Some(end)) => end <= *closing,
            (None, _) | (_, None) => true,
        };
        opens_inside && closes_inside
    }

    /// What remains of the interval from `instant` on, which is where the
    /// rest of a forward page cut at `instant` is read (clause 6.3.10).
    ///
    /// `instant` itself stays in the interval: the instances sharing it may
    /// lie on both sides of the cut, so they are read again rather than
    /// skipped. A closing instant stays where it was, so what remains of a
    /// `before` interval is the `between` interval from `instant` to it.
    #[must_use]
    pub const fn resumed_at(self, instant: DateTime<Utc>) -> TimeInterval {
        match self {
            TimeInterval::Unbounded | TimeInterval::After(_) => TimeInterval::After(instant),
            TimeInterval::Before(to) | TimeInterval::Between { to, .. } => TimeInterval::Between { from: instant, to },
        }
    }

    /// The `timerel`, `timeAt` and `endTimeAt` a request spells the interval
    /// with, the three [`from_parts`](TimeInterval::from_parts) builds it from.
    #[must_use]
    pub const fn parts(self) -> (Option<TimeRel>, Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
        match self {
            TimeInterval::Unbounded => (None, None, None),
            TimeInterval::Before(instant) => (Some(TimeRel::Before), Some(instant), None),
            TimeInterval::After(instant) => (Some(TimeRel::After), Some(instant), None),
            TimeInterval::Between { from, to } => (Some(TimeRel::Between), Some(from), Some(to)),
        }
    }
}

/// The interval of the time axis a temporal query reads, and the property it
/// reads it on (clause 4.5.19).
#[derive(Debug, Clone, Copy, PartialEq, Eq, TypedBuilder)]
pub struct TemporalWindow {
    /// The slice of the time axis the query is bounded to.
    #[builder(default = TimeInterval::Unbounded)]
    pub interval: TimeInterval,
    /// The time property the window applies to.
    pub timeproperty: TimeProp,
}

impl TemporalWindow {
    /// The instant at which an instance placed before the window may still be
    /// in effect, if any.
    ///
    /// Clause 4.11 selects the values "valid within the specified timeframe",
    /// which Annex C clause C.5.5 reads as including the value a stream held
    /// when the window opens. Every instance before a `before` bound lies in
    /// the window already, and an unbounded window holds every instance. On
    /// `deletedAt` the instance in effect at the start is a deletion, which
    /// records no value (clause 4.5.7), so no window on it has one.
    #[must_use]
    pub const fn in_effect_start(&self) -> Option<DateTime<Utc>> {
        match (self.timeproperty, self.interval.opening()) {
            (TimeProp::Deleted, _) | (_, None) => None,
            (TimeProp::Observed | TimeProp::Created | TimeProp::Modified, Some(start)) => Some(*start),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use pretty_assertions::assert_eq;

    fn instant(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("valid instant").get()
    }

    /// Clause 4.11 with Annex C clause C.5.5: only a window with a start, on
    /// an axis other than `deletedAt`, has an instance in effect before it.
    #[test]
    fn only_a_window_with_a_start_off_deleted_at_has_an_in_effect_start() {
        let start = instant("2024-01-01T00:00:00Z");
        let end = instant("2024-02-01T00:00:00Z");
        let window = |timeproperty, interval| TemporalWindow::builder().timeproperty(timeproperty).interval(interval).build();
        for interval in [TimeInterval::After(start), TimeInterval::Between { from: start, to: end }] {
            assert_eq!(window(TimeProp::Observed, interval).in_effect_start(), Some(start), "{interval:?}");
            assert_eq!(window(TimeProp::Modified, interval).in_effect_start(), Some(start), "{interval:?}");
            assert_eq!(window(TimeProp::Deleted, interval).in_effect_start(), None, "{interval:?}");
        }
        for interval in [TimeInterval::Before(end), TimeInterval::Unbounded] {
            assert_eq!(window(TimeProp::Observed, interval).in_effect_start(), None, "{interval:?}");
        }
    }

    #[test]
    fn no_parameters_at_all_is_an_unbounded_interval() {
        assert_eq!(TimeInterval::from_parts(None, None, None).expect("valid"), TimeInterval::Unbounded);
    }

    /// Clause 4.11: `endTimeAt` "is only used for the between relation".
    #[test]
    fn an_end_time_at_beside_any_relation_but_between_is_rejected() {
        let reference = instant("2024-01-01T00:00:00Z");
        let end = instant("2024-06-01T00:00:00Z");
        for timerel in [TimeRel::Before, TimeRel::After] {
            assert!(
                matches!(
                    TimeInterval::from_parts(Some(timerel), Some(reference), Some(end)),
                    Err(QueryError::ParamEndTimeAtWithoutBetween)
                ),
                "{timerel}"
            );
        }
        assert!(matches!(
            TimeInterval::from_parts(None, None, Some(end)),
            Err(QueryError::ParamEndTimeAtWithoutBetween)
        ));
    }

    /// Clause 4.11 makes `timeAt` an inclusive and `endTimeAt` an exclusive
    /// bound, so a `between` closing at or before its opening holds no instant.
    #[test]
    fn a_between_closing_at_or_before_its_opening_is_rejected() {
        let from = instant("2024-01-01T00:00:00.000002Z");
        for to in [from, instant("2024-01-01T00:00:00.000001Z"), instant("2023-12-31T00:00:00Z")] {
            assert!(
                matches!(
                    TimeInterval::from_parts(Some(TimeRel::Between), Some(from), Some(to)),
                    Err(QueryError::ParamEndTimeAtNotAfterTimeAt)
                ),
                "{to}"
            );
        }
        let to = instant("2024-01-01T00:00:00.000003Z");
        assert_eq!(
            TimeInterval::from_parts(Some(TimeRel::Between), Some(from), Some(to)).expect("one microsecond is a window"),
            TimeInterval::Between { from, to }
        );
    }

    /// The pair fault is reported before what the `endTimeAt` is beside.
    #[test]
    fn a_half_specified_pair_is_reported_before_a_stray_end_time_at() {
        let end = instant("2024-06-01T00:00:00Z");
        assert!(matches!(
            TimeInterval::from_parts(Some(TimeRel::Between), None, Some(end)),
            Err(QueryError::ParamTimerelTimeatPair)
        ));
        assert!(matches!(
            TimeInterval::from_parts(None, Some(end), Some(end)),
            Err(QueryError::ParamTimerelTimeatPair)
        ));
    }

    #[test]
    fn between_carries_both_instants_or_is_not_constructed() {
        // Clause 5.7.3: `endTimeAt` is required exactly when `timerel` is
        // `between`, which is why no later stage has to check for it.
        let from = instant("2024-01-01T00:00:00Z");
        let to = instant("2024-06-01T00:00:00Z");
        assert_eq!(
            TimeInterval::from_parts(Some(TimeRel::Between), Some(from), Some(to)).expect("valid"),
            TimeInterval::Between { from, to }
        );
        assert!(matches!(
            TimeInterval::from_parts(Some(TimeRel::Between), Some(from), None),
            Err(QueryError::ParamEndTimeAtRequired)
        ));
    }

    #[test]
    fn a_half_specified_pair_is_rejected() {
        let reference = instant("2024-01-01T00:00:00Z");
        assert!(matches!(
            TimeInterval::from_parts(Some(TimeRel::Before), None, None),
            Err(QueryError::ParamTimerelTimeatPair)
        ));
        assert!(matches!(
            TimeInterval::from_parts(None, Some(reference), None),
            Err(QueryError::ParamTimerelTimeatPair)
        ));
    }

    #[test]
    fn each_relation_opens_and_closes_where_it_bounds_the_axis() {
        let reference = instant("2024-01-01T00:00:00Z");
        let end = instant("2024-01-01T03:00:00Z");
        let between = TimeInterval::Between { from: reference, to: end };
        assert_eq!((TimeInterval::Unbounded.opening(), TimeInterval::Unbounded.closing()), (None, None));
        assert_eq!(
            (TimeInterval::Before(reference).opening(), TimeInterval::Before(reference).closing()),
            (None, Some(&reference))
        );
        assert_eq!(
            (TimeInterval::After(reference).opening(), TimeInterval::After(reference).closing()),
            (Some(&reference), None)
        );
        assert_eq!((between.opening(), between.closing()), (Some(&reference), Some(&end)));
    }

    #[test]
    fn a_resumed_interval_opens_at_the_cut_and_keeps_its_closing_instant() {
        let start = instant("2024-01-01T00:00:00Z");
        let cut = instant("2024-01-02T00:00:00Z");
        let end = instant("2024-02-01T00:00:00Z");
        assert_eq!(TimeInterval::After(start).resumed_at(cut), TimeInterval::After(cut));
        assert_eq!(TimeInterval::Unbounded.resumed_at(cut), TimeInterval::After(cut));
        assert_eq!(TimeInterval::Before(end).resumed_at(cut), TimeInterval::Between { from: cut, to: end });
        assert_eq!(
            TimeInterval::Between { from: start, to: end }.resumed_at(cut),
            TimeInterval::Between { from: cut, to: end }
        );
    }

    #[test]
    fn an_interval_spells_back_the_parts_it_is_built_from() {
        let from = instant("2024-01-01T00:00:00Z");
        let to = instant("2024-02-01T00:00:00Z");
        for interval in [
            TimeInterval::Unbounded,
            TimeInterval::Before(to),
            TimeInterval::After(from),
            TimeInterval::Between { from, to },
        ] {
            let (timerel, time_at, end_time_at) = interval.parts();
            assert_eq!(TimeInterval::from_parts(timerel, time_at, end_time_at).expect("valid parts"), interval);
        }
    }

    /// Clause 4.11: `before` is an exclusive bound, `after` an inclusive one,
    /// and `between` includes its opening and excludes its closing instant.
    #[test]
    fn an_interval_holds_the_instants_of_clause_4_11() {
        let from = instant("2024-01-01T00:00:00Z");
        let to = instant("2024-01-02T00:00:00Z");
        let earlier = instant("2023-12-31T23:59:59Z");
        let inside = instant("2024-01-01T12:00:00Z");
        assert!(TimeInterval::Unbounded.holds(earlier));
        assert!(TimeInterval::Before(to).holds(inside));
        assert!(!TimeInterval::Before(to).holds(to));
        assert!(TimeInterval::After(from).holds(from));
        assert!(!TimeInterval::After(from).holds(earlier));
        let between = TimeInterval::Between { from, to };
        assert!(between.holds(from));
        assert!(between.holds(inside));
        assert!(!between.holds(to));
        assert!(!between.holds(earlier));
    }

    /// A period lies in the interval when it opens at or after its opening
    /// instant and closes at or before its closing instant.
    #[test]
    fn an_interval_holds_the_periods_lying_wholly_in_it() {
        let from = instant("2024-01-01T00:00:00Z");
        let to = instant("2024-01-02T00:00:00Z");
        let hour = |raw: &str| Some(instant(raw));
        let between = TimeInterval::Between { from, to };
        assert!(between.holds_period(hour("2024-01-01T00:00:00Z"), hour("2024-01-01T01:00:00Z")));
        assert!(between.holds_period(hour("2024-01-01T23:00:00Z"), hour("2024-01-02T00:00:00Z")));
        assert!(!between.holds_period(hour("2023-12-31T23:00:00Z"), hour("2024-01-01T00:00:00Z")));
        assert!(!between.holds_period(hour("2024-01-01T23:00:00Z"), hour("2024-01-02T01:00:00Z")));
        assert!(TimeInterval::After(from).holds_period(hour("2024-01-01T00:00:00Z"), hour("2030-01-01T00:00:00Z")));
        assert!(!TimeInterval::Before(to).holds_period(hour("2024-01-01T23:00:00Z"), hour("2024-01-02T01:00:00Z")));
        assert!(between.holds_period(None, hour("2024-01-01T01:00:00Z")));
        assert!(TimeInterval::Unbounded.holds_period(hour("1990-01-01T00:00:00Z"), hour("2090-01-01T00:00:00Z")));
    }

    #[test]
    fn every_bounded_interval_carries_its_reference_instant() {
        let reference = instant("2024-01-01T00:00:00Z");
        assert_eq!(TimeInterval::Unbounded.reference(), None);
        assert_eq!(TimeInterval::Before(reference).reference(), Some(&reference));
    }
}
