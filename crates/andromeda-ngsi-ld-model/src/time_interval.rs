//! The `TimeInterval` member (ETSI GS CIM 009 clause 5.2.11).

use andromeda_ngsi_ld_lexicon::rfc3339;
use andromeda_ngsi_ld_query::temporal::window::TimeInterval as QueryWindow;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A registration's `observationInterval` or `managementInterval`: the span of
/// time the registered source can answer temporal queries for (Table 5.2.11-1).
///
/// `startAt` is mandatory; an absent `endAt` leaves the interval open. A closed
/// interval that ends before it starts describes no time at all, so it is
/// rejected at deserialisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "TimeIntervalWire")]
pub struct TimeInterval {
    /// The start of the interval.
    #[serde(with = "rfc3339::datetime")]
    pub start_at: DateTime<Utc>,
    /// The end of the interval; open when absent.
    #[serde(skip_serializing_if = "Option::is_none", with = "rfc3339::optional_datetime", default)]
    pub end_at: Option<DateTime<Utc>>,
}

impl TimeInterval {
    /// Whether the interval meets a clause 4.11 temporal window, as clauses
    /// 5.10.2.4 and 5.11.2.4 read the match.
    ///
    /// Every relation is the same test: the period the temporal query
    /// describes and the interval the registration declares must share at
    /// least one instant. `before` describes everything up to its instant,
    /// `after` everything from it onwards, `between` the span between its two,
    /// and an absent window every instant there is; an interval without an
    /// `endAt` is open-ended (Table 5.2.11-1) and so has no upper bound to
    /// test.
    ///
    /// Clause 5.11.2.4 states the overlap outright for `between` and words the
    /// other two as "the timeAt ... is contained in or is an endpoint of a time
    /// period included in the specified time interval". Read as "the instant
    /// itself lies inside the interval" that sentence answers the wrong
    /// question: a source covering August 2020 to August 2021 plainly holds
    /// observations made after June 2020, and a query for everything after the
    /// epoch would reach no source at all. The official test material reads it
    /// as the overlap from both directions -- `047_10`/`047_11` expect an
    /// `after` whose instant precedes `startAt` to match, `037_09` a `before`
    /// whose instant follows `endAt` -- and neither is satisfiable under the
    /// containment reading, so the overlap is what is implemented.
    #[must_use]
    pub fn meets(&self, window: &QueryWindow) -> bool {
        let (from, to) = match window {
            QueryWindow::Unbounded => (None, None),
            QueryWindow::Before(instant) => (None, Some(*instant)),
            QueryWindow::After(instant) => (Some(*instant), None),
            QueryWindow::Between { from, to } => (Some(*from), Some(*to)),
        };
        to.is_none_or(|to| self.start_at <= to) && from.is_none_or(|from| self.end_at.is_none_or(|end_at| from <= end_at))
    }
}

/// Why a wire document cannot be a [`TimeInterval`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TimeIntervalError {
    /// `endAt` was not after `startAt`.
    #[error("endAt must be after startAt")]
    EndNotAfterStart,
}

/// The clause 5.2.11 wire shape before the ordering restriction is checked.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimeIntervalWire {
    /// The `startAt` member.
    #[serde(with = "rfc3339::datetime")]
    start_at: DateTime<Utc>,
    /// The `endAt` member.
    #[serde(with = "rfc3339::optional_datetime", default)]
    end_at: Option<DateTime<Utc>>,
}

impl TryFrom<TimeIntervalWire> for TimeInterval {
    type Error = TimeIntervalError;

    fn try_from(wire: TimeIntervalWire) -> Result<TimeInterval, TimeIntervalError> {
        if wire.end_at.is_some_and(|end_at| end_at <= wire.start_at) {
            return Err(TimeIntervalError::EndNotAfterStart);
        }
        Ok(TimeInterval {
            start_at: wire.start_at,
            end_at: wire.end_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::time_interval::TimeInterval;
    use andromeda_ngsi_ld_query::temporal::window::TimeInterval as QueryWindow;
    use serde_json::json;

    #[test]
    fn a_closed_interval_round_trips() {
        let wire = json!({"startAt": "2020-08-01T22:07:00Z", "endAt": "2021-08-01T21:07:00Z"});
        let parsed: TimeInterval = serde_json::from_value(wire.clone()).expect("interval parses"); // clone: compared below
        assert!(parsed.end_at.is_some());
        pretty_assertions::assert_eq!(serde_json::to_value(parsed).expect("serialises"), wire);
    }

    #[test]
    fn an_interval_with_only_start_at_is_open_ended() {
        let parsed: TimeInterval = serde_json::from_value(json!({"startAt": "2020-01-01T00:00:00Z"})).expect("interval parses");
        pretty_assertions::assert_eq!(parsed.end_at, None);
        pretty_assertions::assert_eq!(serde_json::to_value(parsed).expect("serialises"), json!({"startAt": "2020-01-01T00:00:00Z"}));
    }

    #[test]
    fn a_missing_start_at_is_rejected() {
        assert!(serde_json::from_value::<TimeInterval>(json!({"endAt": "2030-01-01T00:00:00Z"})).is_err());
    }

    fn interval(start: &str, end: Option<&str>) -> TimeInterval {
        let mut wire = json!({"startAt": start});
        if let Some(end) = end {
            wire["endAt"] = json!(end);
        }
        serde_json::from_value(wire).expect("interval parses")
    }

    fn at(raw: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(raw).expect("valid instant").with_timezone(&chrono::Utc)
    }

    /// Clauses 5.10.2.4 and 5.11.2.4: `after` reaches every interval that
    /// still holds instants at or beyond it, whether or not the instant itself
    /// lies inside.
    #[test]
    fn after_meets_every_interval_that_ends_at_or_beyond_the_instant() {
        let closed = interval("2020-01-01T00:00:00Z", Some("2021-01-01T00:00:00Z"));
        assert!(closed.meets(&QueryWindow::After(at("2020-06-01T00:00:00Z"))), "an instant inside the interval");
        assert!(closed.meets(&QueryWindow::After(at("2020-01-01T00:00:00Z"))), "the start endpoint counts");
        assert!(closed.meets(&QueryWindow::After(at("2021-01-01T00:00:00Z"))), "the end endpoint counts");
        assert!(
            closed.meets(&QueryWindow::After(at("2019-06-01T00:00:00Z"))),
            "an instant before the start still leaves the whole interval to answer with"
        );
        assert!(
            !closed.meets(&QueryWindow::After(at("2022-06-01T00:00:00Z"))),
            "an instant past the end leaves nothing"
        );
    }

    /// Clauses 5.10.2.4 and 5.11.2.4: `before` reaches every interval that
    /// starts at or before it.
    #[test]
    fn before_meets_every_interval_that_starts_at_or_before_the_instant() {
        let closed = interval("2020-01-01T00:00:00Z", Some("2021-01-01T00:00:00Z"));
        assert!(closed.meets(&QueryWindow::Before(at("2020-06-01T00:00:00Z"))), "an instant inside the interval");
        assert!(closed.meets(&QueryWindow::Before(at("2020-01-01T00:00:00Z"))), "the start endpoint counts");
        assert!(
            closed.meets(&QueryWindow::Before(at("2022-06-01T00:00:00Z"))),
            "an instant past the end still leaves the whole interval to answer with"
        );
        assert!(
            !closed.meets(&QueryWindow::Before(at("2019-01-01T00:00:00Z"))),
            "an instant before the start leaves nothing"
        );
    }

    /// Table 5.2.11-1: an interval without an `endAt` is open-ended, so no
    /// `after` instant can fall past it.
    #[test]
    fn an_open_ended_interval_meets_every_after_window() {
        let open = interval("2020-01-01T00:00:00Z", None);
        assert!(open.meets(&QueryWindow::After(at("2030-01-01T00:00:00Z"))));
        assert!(open.meets(&QueryWindow::After(at("2019-01-01T00:00:00Z"))));
        assert!(!open.meets(&QueryWindow::Before(at("2019-01-01T00:00:00Z"))), "it still starts when it starts");
    }

    /// Clauses 5.10.2.4 and 5.11.2.4: `between` matches on overlap.
    #[test]
    fn between_meets_an_interval_it_overlaps() {
        let closed = interval("2020-01-01T00:00:00Z", Some("2021-01-01T00:00:00Z"));
        let overlapping = QueryWindow::Between {
            from: at("2020-06-01T00:00:00Z"),
            to: at("2020-09-01T00:00:00Z"),
        };
        let straddling = QueryWindow::Between {
            from: at("2019-06-01T00:00:00Z"),
            to: at("2020-02-01T00:00:00Z"),
        };
        let disjoint = QueryWindow::Between {
            from: at("2018-01-01T00:00:00Z"),
            to: at("2019-01-01T00:00:00Z"),
        };
        assert!(closed.meets(&overlapping));
        assert!(closed.meets(&straddling));
        assert!(!closed.meets(&disjoint));
        assert!(closed.meets(&QueryWindow::Unbounded));
    }

    #[test]
    fn an_end_at_not_after_start_at_is_rejected() {
        assert!(serde_json::from_value::<TimeInterval>(json!({"startAt": "2021-01-01T00:00:00Z", "endAt": "2020-01-01T00:00:00Z"})).is_err());
        assert!(serde_json::from_value::<TimeInterval>(json!({"startAt": "2021-01-01T00:00:00Z", "endAt": "2021-01-01T00:00:00Z"})).is_err());
    }
}
