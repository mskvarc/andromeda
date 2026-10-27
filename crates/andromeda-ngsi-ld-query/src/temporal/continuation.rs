//! The request that continues a partial temporal response without serving any
//! instance twice (clause 6.3.10).
//!
//! Clause 6.3.10 says where a page stops but defines no request that resumes
//! after it. A page carries the instances at its edge instant whole, and
//! clause 4.11 makes `after` and the `between` opening inclusive, so a forward
//! continuation opening at the edge would serve that instant again. It opens
//! one microsecond past it instead: stored instants are microsecond precision,
//! and clause 4.6.3 instants with a finer fraction are refused, so no instance
//! lies between the edge and that opening. `before` and the `between` closing
//! are exclusive, so a backward continuation closes at the edge itself.
//!
//! An aggregated page ends on a period boundary, which the next period opens
//! at (clause 4.5.19), so a forward continuation opens on the boundary itself.
//! The periods are laid from the window's reference instant, and a calendar
//! period's boundary is the origin moved by whole durations with the day of
//! month clamped, so a grid laid from a later boundary can differ from the one
//! laid from the origin: from 29 February, `P1M` falls on the 29th where the
//! grid from 31 January falls on each month's last day. A forward continuation
//! of a calendar grid therefore names the origin it keeps
//! (`aggrPeriodOrigin`). An exact grid laid from any of its boundaries is the
//! same grid.
//!
//! A backward page answers a `lastN` the broker's maximum narrowed, and the
//! continuation asks for the rest of it: the client's `lastN` less the fewest
//! instances any attribute stream received. A stream that received more is
//! served past its `lastN` by at most that difference.

use crate::temporal::{instance_page::PageDirection, window::TimeInterval};
use chrono::{DateTime, TimeDelta, Utc};
use std::num::NonZeroU32;

/// What a partial temporal page ends on, in the direction it is paged in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageEdge {
    /// An instance page: the most recent instant it serves forwards, or the
    /// least recent backwards, with every instance at that instant served.
    Instant(DateTime<Utc>),
    /// An aggregated page: the end of its last period forwards, or the start
    /// of its oldest period backwards.
    PeriodBoundary {
        /// The boundary the page ends on.
        boundary: DateTime<Utc>,
        /// The origin of the calendar grid the periods lie on; `None` on an
        /// exact grid.
        origin: Option<DateTime<Utc>>,
    },
}

impl PageEdge {
    /// The instant the edge lies at.
    #[must_use]
    pub const fn instant(self) -> DateTime<Utc> {
        match self {
            PageEdge::Instant(instant) | PageEdge::PeriodBoundary { boundary: instant, .. } => instant,
        }
    }

    /// The instant a forward continuation opens at, or `None` when the time
    /// axis ends before it.
    fn forward_opening(self) -> Option<DateTime<Utc>> {
        match self {
            PageEdge::Instant(instant) => instant.checked_add_signed(TimeDelta::microseconds(1)),
            PageEdge::PeriodBoundary { boundary, .. } => Some(boundary),
        }
    }

    /// The origin a forward continuation lays its periods from.
    const fn calendar_origin(self) -> Option<DateTime<Utc>> {
        match self {
            PageEdge::Instant(_) => None,
            PageEdge::PeriodBoundary { origin, .. } => origin,
        }
    }
}

/// The window and `lastN` of the request that continues a partial temporal
/// response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemporalContinuation {
    /// The window of the time axis the continuation reads.
    pub interval: TimeInterval,
    /// The `lastN` the continuation asks for; `None` on a forward walk, which
    /// carries none.
    pub last_n: Option<NonZeroU32>,
    /// The origin a forward continuation of a calendar grid lays its periods
    /// from; `None` for any other continuation, which keeps the request's.
    pub period_origin: Option<DateTime<Utc>>,
}

impl TemporalContinuation {
    /// The continuation of a page of `interval` paged in `direction` and
    /// ending on `edge`, or `None` when nothing of the request remains.
    ///
    /// `fewest_served` is the fewest instances any attribute stream of the
    /// response received; a backward page whose count is unknown has no
    /// continuation, since the rest of its `lastN` cannot be stated.
    #[must_use]
    pub fn following(interval: TimeInterval, direction: PageDirection, edge: PageEdge, fewest_served: Option<u64>) -> Option<TemporalContinuation> {
        match direction {
            PageDirection::Forward => {
                let opening = edge.forward_opening()?;
                let interval = match interval {
                    TimeInterval::Unbounded | TimeInterval::After(_) => TimeInterval::After(opening),
                    TimeInterval::Before(to) | TimeInterval::Between { to, .. } if opening < to => TimeInterval::Between { from: opening, to },
                    TimeInterval::Before(_) | TimeInterval::Between { .. } => return None,
                };
                Some(TemporalContinuation {
                    interval,
                    last_n: None,
                    period_origin: edge.calendar_origin(),
                })
            }
            PageDirection::Backward { requested } => {
                let remaining = u64::from(requested.get()).checked_sub(fewest_served?)?;
                let last_n = NonZeroU32::new(u32::try_from(remaining).ok()?)?;
                let closing = edge.instant();
                let interval = match interval {
                    TimeInterval::Unbounded | TimeInterval::Before(_) => TimeInterval::Before(closing),
                    TimeInterval::After(from) | TimeInterval::Between { from, .. } if from < closing => TimeInterval::Between { from, to: closing },
                    TimeInterval::After(_) | TimeInterval::Between { .. } => return None,
                };
                Some(TemporalContinuation {
                    interval,
                    last_n: Some(last_n),
                    period_origin: None,
                })
            }
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::temporal::{
        continuation::{PageEdge, TemporalContinuation},
        instance_page::PageDirection,
        window::TimeInterval,
    };
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU32;

    fn at(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("a valid instant").get()
    }

    fn forward(interval: TimeInterval, edge: PageEdge) -> Option<TemporalContinuation> {
        TemporalContinuation::following(interval, PageDirection::Forward, edge, None)
    }

    /// A backward page of a `lastN` of 5000 whose sparsest stream received
    /// `fewest` instances.
    fn backward(interval: TimeInterval, edge: PageEdge, fewest: Option<u64>) -> Option<TemporalContinuation> {
        let direction = PageDirection::Backward {
            requested: NonZeroU32::new(5000).expect("a positive lastN"),
        };
        TemporalContinuation::following(interval, direction, edge, fewest)
    }

    fn edge_0119() -> PageEdge {
        PageEdge::Instant(at("2020-01-01T01:19:00Z"))
    }

    fn edge_0101() -> PageEdge {
        PageEdge::Instant(at("2020-01-01T01:01:00Z"))
    }

    fn last_n(n: u32) -> Option<NonZeroU32> {
        NonZeroU32::new(n)
    }

    #[test]
    fn a_forward_after_page_continues_one_microsecond_past_its_edge() {
        assert_eq!(
            forward(TimeInterval::After(at("2020-01-01T00:00:00Z")), edge_0119()),
            Some(TemporalContinuation {
                interval: TimeInterval::After(at("2020-01-01T01:19:00.000001Z")),
                last_n: None,
                period_origin: None,
            })
        );
    }

    #[test]
    fn a_forward_between_page_continues_past_its_edge_to_the_same_closing_instant() {
        let interval = TimeInterval::Between {
            from: at("2020-01-01T00:00:00Z"),
            to: at("2020-01-01T02:00:00Z"),
        };
        assert_eq!(
            forward(interval, edge_0119()).map(|next| next.interval),
            Some(TimeInterval::Between {
                from: at("2020-01-01T01:19:00.000001Z"),
                to: at("2020-01-01T02:00:00Z"),
            })
        );
    }

    /// What remains of a `before` window past the edge is the `between`
    /// window from just past the edge to its closing instant.
    #[test]
    fn a_forward_before_page_continues_as_a_between_window() {
        assert_eq!(
            forward(TimeInterval::Before(at("2020-01-01T02:00:00Z")), edge_0119()).map(|next| next.interval),
            Some(TimeInterval::Between {
                from: at("2020-01-01T01:19:00.000001Z"),
                to: at("2020-01-01T02:00:00Z"),
            })
        );
    }

    #[test]
    fn a_forward_unbounded_page_continues_as_an_after_window() {
        assert_eq!(
            forward(TimeInterval::Unbounded, edge_0119()).map(|next| next.interval),
            Some(TimeInterval::After(at("2020-01-01T01:19:00.000001Z")))
        );
    }

    /// The window closes exactly where the continuation would open, so no
    /// instant of it remains.
    #[test]
    fn a_forward_page_ending_one_microsecond_before_the_closing_instant_has_no_continuation() {
        let interval = TimeInterval::Between {
            from: at("2020-01-01T00:00:00Z"),
            to: at("2020-01-01T01:19:00.000001Z"),
        };
        assert_eq!(forward(interval, edge_0119()), None);
    }

    /// Clause 4.5.19: the next period opens on the boundary the page ends on.
    #[test]
    fn a_forward_aggregated_page_continues_on_its_period_boundary() {
        assert_eq!(
            forward(
                TimeInterval::After(at("2020-01-01T00:00:00Z")),
                PageEdge::PeriodBoundary {
                    boundary: at("2020-01-01T19:00:00Z"),
                    origin: None,
                }
            )
            .map(|next| next.interval),
            Some(TimeInterval::After(at("2020-01-01T19:00:00Z")))
        );
    }

    /// From 29 February a grid of `P1M` falls on the 29th, so the
    /// continuation keeps the 31 January origin it was laid from.
    #[test]
    fn a_forward_calendar_page_continues_on_its_boundary_keeping_its_origin() {
        assert_eq!(
            forward(
                TimeInterval::After(at("2020-01-31T00:00:00Z")),
                PageEdge::PeriodBoundary {
                    boundary: at("2020-02-29T00:00:00Z"),
                    origin: Some(at("2020-01-31T00:00:00Z")),
                }
            ),
            Some(TemporalContinuation {
                interval: TimeInterval::After(at("2020-02-29T00:00:00Z")),
                last_n: None,
                period_origin: Some(at("2020-01-31T00:00:00Z")),
            })
        );
    }

    /// 5000 requested, the sparsest stream received 1000, so 4000 remain.
    #[test]
    fn a_backward_before_page_continues_before_its_edge_with_the_rest_of_its_last_n() {
        assert_eq!(
            backward(TimeInterval::Before(at("2020-01-01T02:00:00Z")), edge_0101(), Some(1000)),
            Some(TemporalContinuation {
                interval: TimeInterval::Before(at("2020-01-01T01:01:00Z")),
                last_n: last_n(4000),
                period_origin: None,
            })
        );
    }

    #[test]
    fn a_backward_between_page_continues_from_the_same_opening_to_its_edge() {
        let interval = TimeInterval::Between {
            from: at("2020-01-01T00:00:00Z"),
            to: at("2020-01-01T02:00:00Z"),
        };
        assert_eq!(
            backward(interval, edge_0101(), Some(1000)),
            Some(TemporalContinuation {
                interval: TimeInterval::Between {
                    from: at("2020-01-01T00:00:00Z"),
                    to: at("2020-01-01T01:01:00Z"),
                },
                last_n: last_n(4000),
                period_origin: None,
            })
        );
    }

    #[test]
    fn a_backward_after_page_continues_as_a_between_window() {
        assert_eq!(
            backward(TimeInterval::After(at("2020-01-01T00:00:00Z")), edge_0101(), Some(1000)),
            Some(TemporalContinuation {
                interval: TimeInterval::Between {
                    from: at("2020-01-01T00:00:00Z"),
                    to: at("2020-01-01T01:01:00Z"),
                },
                last_n: last_n(4000),
                period_origin: None,
            })
        );
    }

    #[test]
    fn a_backward_unbounded_page_continues_as_a_before_window() {
        assert_eq!(
            backward(TimeInterval::Unbounded, edge_0101(), Some(1000)),
            Some(TemporalContinuation {
                interval: TimeInterval::Before(at("2020-01-01T01:01:00Z")),
                last_n: last_n(4000),
                period_origin: None,
            })
        );
    }

    /// A stream the page served nothing of still owes the whole `lastN`.
    #[test]
    fn a_backward_page_leaving_a_stream_empty_continues_with_the_whole_last_n() {
        assert_eq!(
            backward(TimeInterval::Unbounded, edge_0101(), Some(0)).and_then(|next| next.last_n),
            last_n(5000)
        );
    }

    #[test]
    fn a_backward_page_serving_every_stream_its_whole_last_n_has_no_continuation() {
        assert_eq!(backward(TimeInterval::Unbounded, edge_0101(), Some(5000)), None);
        assert_eq!(backward(TimeInterval::Unbounded, edge_0101(), Some(7000)), None);
    }

    #[test]
    fn a_backward_page_whose_fewest_served_is_unknown_has_no_continuation() {
        assert_eq!(backward(TimeInterval::Unbounded, edge_0101(), None), None);
    }

    /// The oldest kept period starts on the boundary, which an exclusive
    /// `before` leaves to the page already served. A backward continuation
    /// names no origin of its own, whatever grid the page lies on.
    #[test]
    fn a_backward_aggregated_page_continues_before_its_period_boundary() {
        assert_eq!(
            backward(
                TimeInterval::Before(at("2020-01-02T00:00:00Z")),
                PageEdge::PeriodBoundary {
                    boundary: at("2020-01-01T10:00:00Z"),
                    origin: Some(at("2020-01-02T00:00:00Z")),
                },
                Some(1000)
            ),
            Some(TemporalContinuation {
                interval: TimeInterval::Before(at("2020-01-01T10:00:00Z")),
                last_n: last_n(4000),
                period_origin: None,
            })
        );
    }

    #[test]
    fn a_forward_page_ending_at_the_last_representable_instant_has_no_continuation() {
        assert_eq!(forward(TimeInterval::Unbounded, PageEdge::Instant(DateTime::<Utc>::MAX_UTC)), None);
    }

    /// The window opens at the edge, so nothing of it lies before the edge.
    #[test]
    fn a_backward_page_ending_on_the_opening_instant_has_no_continuation() {
        assert_eq!(backward(TimeInterval::After(at("2020-01-01T01:01:00Z")), edge_0101(), Some(1000)), None);
    }
}
