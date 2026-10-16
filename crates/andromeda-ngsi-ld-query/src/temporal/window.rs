//! The interval of the time axis a query reads.

use crate::error::QueryError;
use andromeda_ngsi_ld_lexicon::{time_prop::TimeProp, time_rel::TimeRel};
use chrono::{DateTime, Utc};
use typed_builder::TypedBuilder;

/// The slice of the time axis a query is bounded to (clause 4.11).
///
/// `timerel` and `timeAt` are meaningless apart — clause 5.7.3 requires both or
/// neither — and `endTimeAt` is required exactly when `timerel` is `between`.
/// Stating that in the type means SQL builders need no runtime checks for it,
/// and none has to raise a *request* error from inside a statement builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeInterval {
    /// The query bounded nothing; every instant qualifies.
    Unbounded,
    /// Everything strictly before the instant.
    Before(DateTime<Utc>),
    /// Everything at or after the instant.
    After(DateTime<Utc>),
    /// Everything from `from` inclusive to `to` exclusive.
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
    /// # Errors
    ///
    /// Returns [`QueryError::ParamTimerelTimeatPair`] when exactly one of
    /// `timerel` and `timeAt` is present, and
    /// [`QueryError::ParamEndTimeAtRequired`] when `timerel` is `between` and
    /// no `endTimeAt` came with it.
    pub const fn from_parts(timerel: Option<TimeRel>, time_at: Option<DateTime<Utc>>, end_time_at: Option<DateTime<Utc>>) -> Result<TimeInterval, QueryError> {
        match (timerel, time_at) {
            (None, None) => Ok(TimeInterval::Unbounded),
            (Some(TimeRel::Before), Some(instant)) => Ok(TimeInterval::Before(instant)),
            (Some(TimeRel::After), Some(instant)) => Ok(TimeInterval::After(instant)),
            (Some(TimeRel::Between), Some(from)) => match end_time_at {
                Some(to) => Ok(TimeInterval::Between { from, to }),
                None => Err(QueryError::ParamEndTimeAtRequired),
            },
            (None, Some(_)) | (Some(_), None) => Err(QueryError::ParamTimerelTimeatPair),
        }
    }

    /// Reject the `timerel`/`timeAt`/`endTimeAt` combinations clause 5.7.3
    /// forbids, from the presence of each parameter alone.
    ///
    /// The two pair-faults belong to this module: [`from_parts`](TimeInterval::from_parts)
    /// raises them when it builds an interval from parsed values, and a request
    /// layer that has only established which parameters arrived — not yet parsed
    /// them — asks here rather than re-deriving the same rule and minting the
    /// same errors at the request seam, where they had drifted out of step with
    /// this one (a CSR check that rejected `timerel` without `timeAt` but let
    /// `timeAt` without `timerel` through).
    ///
    /// `timerel` is the relation the request carried, if any; the other two
    /// parameters are reported as presence alone because the request seam has
    /// not parsed their instants yet.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::ParamTimerelTimeatPair`] when exactly one of
    /// `timerel` and `timeAt` is present, and
    /// [`QueryError::ParamEndTimeAtRequired`] when `timerel` is `between` and no
    /// `endTimeAt` came with it.
    pub const fn check_time_bounds(timerel: Option<TimeRel>, time_at: BoundPresence, end_time_at: BoundPresence) -> Result<(), QueryError> {
        match (timerel, time_at) {
            (Some(_), BoundPresence::Absent) | (None, BoundPresence::Present) => return Err(QueryError::ParamTimerelTimeatPair),
            (Some(_), BoundPresence::Present) | (None, BoundPresence::Absent) => (),
        }
        match (timerel, end_time_at) {
            (Some(TimeRel::Between), BoundPresence::Absent) => Err(QueryError::ParamEndTimeAtRequired),
            (Some(TimeRel::Before | TimeRel::After | TimeRel::Between) | None, BoundPresence::Present)
            | (Some(TimeRel::Before | TimeRel::After) | None, BoundPresence::Absent) => Ok(()),
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

/// Whether an optional temporal bound arrived in the request.
///
/// The request seam establishes which parameters were sent before it parses
/// them, so the pair rule of clause 5.7.3 is checked on presence alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundPresence {
    /// The parameter was carried by the request.
    Present,
    /// The parameter was not carried by the request.
    Absent,
}

impl BoundPresence {
    /// Read the presence of an optional parameter.
    #[must_use]
    pub const fn of<T>(value: Option<&T>) -> BoundPresence {
        match value {
            Some(_) => BoundPresence::Present,
            None => BoundPresence::Absent,
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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn instant(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("valid instant").with_timezone(&Utc)
    }

    #[test]
    fn no_parameters_at_all_is_an_unbounded_interval() {
        assert_eq!(TimeInterval::from_parts(None, None, None).expect("valid"), TimeInterval::Unbounded);
    }

    #[test]
    fn check_time_bounds_is_the_presence_only_reading_of_the_pair_rule() {
        // The request seam calls this before it has parsed instants, so it
        // matches `from_parts` on presence alone: both `timerel` and `timeAt`,
        // or neither, and `endTimeAt` present when `timerel` is `between`.
        assert!(TimeInterval::check_time_bounds(None, BoundPresence::Absent, BoundPresence::Absent).is_ok());
        assert!(TimeInterval::check_time_bounds(Some(TimeRel::After), BoundPresence::Present, BoundPresence::Absent).is_ok());
        assert!(TimeInterval::check_time_bounds(Some(TimeRel::Between), BoundPresence::Present, BoundPresence::Present).is_ok());
        // Either half alone is rejected, in both directions.
        assert!(matches!(
            TimeInterval::check_time_bounds(Some(TimeRel::After), BoundPresence::Absent, BoundPresence::Absent),
            Err(QueryError::ParamTimerelTimeatPair)
        ));
        assert!(matches!(
            TimeInterval::check_time_bounds(None, BoundPresence::Present, BoundPresence::Absent),
            Err(QueryError::ParamTimerelTimeatPair)
        ));
        // `between` without an `endTimeAt` is the second fault.
        assert!(matches!(
            TimeInterval::check_time_bounds(Some(TimeRel::Between), BoundPresence::Present, BoundPresence::Absent),
            Err(QueryError::ParamEndTimeAtRequired)
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

    #[test]
    fn every_bounded_interval_carries_its_reference_instant() {
        let reference = instant("2024-01-01T00:00:00Z");
        assert_eq!(TimeInterval::Unbounded.reference(), None);
        assert_eq!(TimeInterval::Before(reference).reference(), Some(&reference));
    }
}
