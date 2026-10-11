//! The periods the time range of an aggregated temporal query divides into
//! (clause 4.5.19).
//!
//! Clause 4.5.19 makes each aggregation method "a JSON-LD Array that shall
//! contain as many array elements as there are periods in the time range of the
//! query", while the store answers only the periods that hold an Attribute
//! instance. The timeline lays the empty periods out around the held ones:
//! before the first back to the query's opening instant, between two held
//! periods, and after the last up to the query's closing instant. A side the
//! query leaves open ends at the outermost held period, the only bound there is.
//!
//! Every empty period is stepped from a bound the store computed, so the held
//! and the empty periods share one grid without the timeline knowing how the
//! store aligns it. The count is taken from the stride arithmetic before any
//! period is materialised, so a query asking for more periods than the broker
//! answers is refused without building them.

use crate::temporal::{compiled::CompiledTemporalQuery, period_stride::PeriodStride, window::TimeInterval};
use chrono::{DateTime, Utc};
use std::iter::successors;
use thiserror::Error;

/// The periods of an aggregated query's time range, and how many of them one
/// aggregated attribute may answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeriodTimeline {
    /// How far each period reaches from its start.
    stride: PeriodStride,
    /// The query's opening instant, `None` when the range reaches back
    /// without limit.
    opening: Option<DateTime<Utc>>,
    /// The query's closing instant, `None` when the range reaches forward
    /// without limit.
    closing: Option<DateTime<Utc>>,
    /// The most periods one aggregated attribute answers.
    maximum: u64,
}

/// One period of the timeline: one the store answered, or one that holds no
/// Attribute instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelinePeriod<'h, T> {
    /// A period the store answered.
    Held(&'h T),
    /// A period holding no Attribute instance.
    Empty {
        /// The start of the period.
        start: DateTime<Utc>,
        /// The end of the period.
        end: DateTime<Utc>,
    },
}

/// The time range divides into more periods than one aggregated attribute
/// answers.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
#[error("the time range divides into {periods} aggregation periods, over the maximum of {maximum}")]
pub struct PeriodLimitExceeded {
    /// The periods the range divides into, saturated at `u64::MAX` when the
    /// count leaves the representable time axis.
    pub periods: u64,
    /// The most periods one aggregated attribute answers.
    pub maximum: u64,
}

/// A stretch of empty periods, stepped from `from`.
#[derive(Debug, Clone, Copy)]
struct Gap {
    /// The start of the first empty period.
    from: DateTime<Utc>,
    /// How many empty periods the stretch holds.
    count: u64,
}

/// A held period or a stretch of empty ones, in timeline order.
enum Segment<'h, T> {
    /// A period the store answered.
    Held(&'h T),
    /// Empty periods.
    Gap(Gap),
}

impl PeriodTimeline {
    /// The timeline of `compiled`, answering at most `maximum` periods per
    /// aggregated attribute. `None` when the query aggregates the whole range
    /// into one period, or names a width the store cannot bucket on.
    #[must_use]
    pub fn of(compiled: &CompiledTemporalQuery, maximum: u64) -> Option<PeriodTimeline> {
        let stride = compiled.representation.aggr_period_duration?.stride()?;
        Some(PeriodTimeline::new(stride, compiled.window.interval, maximum))
    }

    /// The timeline of `interval` stepped by `stride`, answering at most
    /// `maximum` periods per aggregated attribute.
    #[must_use]
    pub const fn new(stride: PeriodStride, interval: TimeInterval, maximum: u64) -> PeriodTimeline {
        PeriodTimeline {
            stride,
            opening: interval.opening().copied(),
            closing: interval.closing().copied(),
            maximum,
        }
    }

    /// `held`, in period order, with every empty period of the range laid out
    /// around it. `bounds` reads a held period's start and end; one without
    /// bounds stays where it is and opens no gap.
    ///
    /// # Errors
    ///
    /// Returns [`PeriodLimitExceeded`] when the range divides into more
    /// periods than the timeline answers.
    pub fn lay_out<'h, T>(
        &self,
        held: &'h [T],
        bounds: impl Fn(&T) -> Option<(DateTime<Utc>, DateTime<Utc>)>,
    ) -> Result<Vec<TimelinePeriod<'h, T>>, PeriodLimitExceeded> {
        let segments = self.segments(held, bounds);
        let periods = segments.iter().fold(0_u64, |total, segment| {
            total.saturating_add(match segment {
                Segment::Held(_) => 1,
                Segment::Gap(gap) => gap.count,
            })
        });
        if periods > self.maximum {
            return Err(PeriodLimitExceeded {
                periods,
                maximum: self.maximum,
            });
        }
        let mut laid_out = Vec::with_capacity(usize::try_from(periods).unwrap_or(0));
        for segment in segments {
            match segment {
                Segment::Held(period) => laid_out.push(TimelinePeriod::Held(period)),
                Segment::Gap(gap) => laid_out.extend(self.empty_periods(gap)),
            }
        }
        Ok(laid_out)
    }

    /// The held periods and the gaps around them, before any empty period is
    /// built. A gap whose count leaves the time axis saturates, so the count
    /// check refuses it.
    fn segments<'h, T>(&self, held: &'h [T], bounds: impl Fn(&T) -> Option<(DateTime<Utc>, DateTime<Utc>)>) -> Vec<Segment<'h, T>> {
        let mut segments = Vec::with_capacity(held.len().saturating_mul(2).saturating_add(1));
        let mut previous_end: Option<DateTime<Utc>> = None;
        for period in held {
            if let Some((start, end)) = bounds(period) {
                let gap = match previous_end {
                    Some(previous_end) => Some(Gap {
                        from: previous_end,
                        count: self.stride.periods_forward(previous_end, start).unwrap_or(u64::MAX),
                    }),
                    None => self.opening.map(|opening| self.leading_gap(opening, start)),
                };
                segments.extend(gap.map(Segment::Gap));
                previous_end = Some(end);
            }
            segments.push(Segment::Held(period));
        }
        if let (Some(previous_end), Some(closing)) = (previous_end, self.closing) {
            segments.push(Segment::Gap(Gap {
                from: previous_end,
                count: self.stride.periods_forward(previous_end, closing).unwrap_or(u64::MAX),
            }));
        }
        segments
    }

    /// The empty periods from the one holding `opening` up to `first`, the
    /// start of the first held period.
    fn leading_gap(&self, opening: DateTime<Utc>, first: DateTime<Utc>) -> Gap {
        self.stride
            .periods_backward(first, opening)
            .and_then(|count| self.stride.retreat(first, count).map(|from| Gap { from, count }))
            .unwrap_or(Gap { from: first, count: u64::MAX })
    }

    /// The empty periods of one gap, each starting where the last ended.
    fn empty_periods<'h, T: 'h>(&self, gap: Gap) -> impl Iterator<Item = TimelinePeriod<'h, T>> {
        let stride = self.stride;
        successors(Some(gap.from), move |start| stride.advance(*start, 1))
            .take(usize::try_from(gap.count).unwrap_or(usize::MAX))
            .map_while(move |start| stride.advance(start, 1).map(|end| TimelinePeriod::Empty { start, end }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;
    use pretty_assertions::assert_eq;

    fn instant(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("valid instant").with_timezone(&Utc)
    }

    fn hourly(opening: Option<&str>, closing: Option<&str>, maximum: u64) -> PeriodTimeline {
        PeriodTimeline {
            stride: PeriodStride::Fixed(TimeDelta::hours(1)),
            opening: opening.map(instant),
            closing: closing.map(instant),
            maximum,
        }
    }

    /// A held period as the store answers it: its start and end.
    fn held(start: &str, end: &str) -> (DateTime<Utc>, DateTime<Utc>) {
        (instant(start), instant(end))
    }

    /// The start and end of every laid-out period, `true` for a held one.
    fn shape(periods: &[TimelinePeriod<'_, (DateTime<Utc>, DateTime<Utc>)>]) -> Vec<(DateTime<Utc>, DateTime<Utc>, bool)> {
        periods
            .iter()
            .map(|period| match period {
                TimelinePeriod::Held((start, end)) => (*start, *end, true),
                TimelinePeriod::Empty { start, end } => (*start, *end, false),
            })
            .collect()
    }

    fn period(start: &str, end: &str, is_held: bool) -> (DateTime<Utc>, DateTime<Utc>, bool) {
        (instant(start), instant(end), is_held)
    }

    /// Clause 4.5.19: one element per period of the query's range, so the
    /// empty hour between two held ones is laid out.
    #[test]
    fn an_empty_period_between_two_held_ones_is_laid_out() {
        let timeline = hourly(Some("2024-01-01T00:00:00Z"), Some("2024-01-01T03:00:00Z"), 100);
        let rows = [
            held("2024-01-01T00:00:00Z", "2024-01-01T01:00:00Z"),
            held("2024-01-01T02:00:00Z", "2024-01-01T03:00:00Z"),
        ];
        let periods = timeline.lay_out(&rows, |row| Some(*row)).expect("within the maximum");
        assert_eq!(
            shape(&periods),
            vec![
                period("2024-01-01T00:00:00Z", "2024-01-01T01:00:00Z", true),
                period("2024-01-01T01:00:00Z", "2024-01-01T02:00:00Z", false),
                period("2024-01-01T02:00:00Z", "2024-01-01T03:00:00Z", true),
            ]
        );
    }

    #[test]
    fn the_range_is_filled_back_to_the_opening_and_on_to_the_closing_instant() {
        let timeline = hourly(Some("2024-01-01T00:00:00Z"), Some("2024-01-01T04:00:00Z"), 100);
        let rows = [held("2024-01-01T02:00:00Z", "2024-01-01T03:00:00Z")];
        let periods = timeline.lay_out(&rows, |row| Some(*row)).expect("within the maximum");
        assert_eq!(
            shape(&periods),
            vec![
                period("2024-01-01T00:00:00Z", "2024-01-01T01:00:00Z", false),
                period("2024-01-01T01:00:00Z", "2024-01-01T02:00:00Z", false),
                period("2024-01-01T02:00:00Z", "2024-01-01T03:00:00Z", true),
                period("2024-01-01T03:00:00Z", "2024-01-01T04:00:00Z", false),
            ]
        );
    }

    /// A side the query leaves open has no instant to fill to, so the range
    /// ends at the outermost held period.
    #[test]
    fn an_open_side_ends_at_the_outermost_held_period() {
        let timeline = hourly(None, None, 100);
        let rows = [
            held("2024-01-01T00:00:00Z", "2024-01-01T01:00:00Z"),
            held("2024-01-01T02:00:00Z", "2024-01-01T03:00:00Z"),
        ];
        let periods = timeline.lay_out(&rows, |row| Some(*row)).expect("within the maximum");
        assert_eq!(periods.len(), 3);
    }

    /// The period holding the opening instant starts on the grid the store
    /// aligned the held periods to, not at the opening instant itself.
    #[test]
    fn the_leading_period_is_the_one_holding_the_opening_instant() {
        let timeline = PeriodTimeline {
            stride: PeriodStride::Months(1),
            opening: Some(instant("2024-01-15T10:00:00Z")),
            closing: None,
            maximum: 100,
        };
        let rows = [held("2024-03-01T00:00:00Z", "2024-04-01T00:00:00Z")];
        let periods = timeline.lay_out(&rows, |row| Some(*row)).expect("within the maximum");
        assert_eq!(
            shape(&periods),
            vec![
                period("2024-01-01T00:00:00Z", "2024-02-01T00:00:00Z", false),
                period("2024-02-01T00:00:00Z", "2024-03-01T00:00:00Z", false),
                period("2024-03-01T00:00:00Z", "2024-04-01T00:00:00Z", true),
            ]
        );
    }

    #[test]
    fn a_range_of_more_periods_than_the_maximum_is_refused_unbuilt() {
        let timeline = hourly(Some("2024-01-01T00:00:00Z"), Some("2124-01-01T00:00:00Z"), 1000);
        let rows = [held("2024-01-01T00:00:00Z", "2024-01-01T01:00:00Z")];
        assert_eq!(
            timeline.lay_out(&rows, |row| Some(*row)),
            Err(PeriodLimitExceeded {
                periods: 876_576,
                maximum: 1000,
            })
        );
    }

    #[test]
    fn no_held_period_lays_out_nothing() {
        let timeline = hourly(Some("2024-01-01T00:00:00Z"), Some("2024-01-01T03:00:00Z"), 100);
        let rows: [(DateTime<Utc>, DateTime<Utc>); 0] = [];
        assert_eq!(timeline.lay_out(&rows, |row| Some(*row)).expect("within the maximum").len(), 0);
    }
}
