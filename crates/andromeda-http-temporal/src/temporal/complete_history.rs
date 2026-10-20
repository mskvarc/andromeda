//! How far the merged history of a federated temporal read is complete
//! (clause 6.3.10).
//!
//! The local read and every context source each answer with as much of the
//! history as they can at once, and one that stops short says where: the
//! local read by the cut it made, a source by the `Content-Range` of its 206.
//! Clause 6.3.10 gives the response a single `Content-Range` whose range-end
//! is "the most recent timestamp in the range of the response" when paging
//! forwards and "the least recent timestamp" when paging backwards with
//! `lastN`, and the client resumes from it. So the merged page is complete
//! only up to the earliest of those stops forwards, and down to the latest of
//! them backwards; instances past that bound are left out rather than
//! answered under a range that claims the ones before it are all there, and
//! the continuation fetches them again from wherever they are held.
//!
//! The local page carries every instance at its edge instant, so its stop is
//! inclusive. A source's range-end is the last instant it served, but nothing
//! says it served every instance at that instant, so a source's stop is
//! exclusive: its instances at that instant are left to the continuation,
//! which opens just past the last instant kept. When leaving them out would
//! leave the page no instance at all, the page could not advance, so the stop
//! is taken inclusively and the source is reported (clause 6.3.17): what it
//! held at that instant beyond its page is not reached. An aggregated period
//! is whole, so every stop of an aggregated merge is inclusive.

use crate::temporal::TemporalQueryParameters;
use andromeda_federation::{incomplete_answer::history_cut_short, outcome::ForwardedTemporalEntities};
use andromeda_http_headers::{ngsild_content_range::NgsiLdContentRange, ngsild_warning::NgsiLdWarning};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time_prop::TimeProp};
use andromeda_ngsi_ld_model::temporal::temporal_history::TemporalHistory;
use andromeda_ngsi_ld_query::temporal::{
    continuation::{PageEdge, TemporalContinuation},
    instance_page::PageDirection,
    window::TimeInterval,
};
use andromeda_temporal::{error::TemporalError, service::TemporalRange};
use chrono::{DateTime, Utc};

/// How complete the history a federated temporal read answers with is.
#[derive(Debug)]
pub(crate) enum HistoryCompleteness {
    /// No context source contributed, so the local read's own range, if it
    /// cut the history short, is the response's.
    Local(Option<TemporalRange>),
    /// Context sources contributed, and every contributor answered in full.
    Complete,
    /// Context sources contributed, and at least one contributor stopped short.
    UpTo(HistoryBound),
}

/// Whether the instances at a stop's instant are on the complete side of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopSide {
    /// Every instance at the instant was served.
    Inclusive,
    /// Some instance at the instant may not have been served.
    Exclusive,
}

/// Where one contributor stopped short, in the direction the response is
/// paged in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stop {
    /// The instant the contributor stopped at.
    at: DateTime<Utc>,
    /// Whether the instances at that instant were all served.
    side: StopSide,
}

impl Stop {
    /// The tighter of two stops: forwards the earlier, backwards the later,
    /// and of two at one instant the exclusive one, which admits less.
    fn tighter(self, other: Stop, direction: PageDirection) -> Stop {
        let other_first = match direction {
            PageDirection::Forward => other.at < self.at,
            PageDirection::Backward { .. } => other.at > self.at,
        };
        if other_first || (other.at == self.at && other.side == StopSide::Exclusive) {
            other
        } else {
            self
        }
    }
}

/// What the merged histories are made of, which decides what a page of them
/// ends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergedForm {
    /// Attribute instances, each at an instant.
    Instances,
    /// Aggregated periods, each whole (clause 4.5.19).
    Periods,
}

impl MergedForm {
    /// The form of `histories`: a request answers in one representation, so
    /// one aggregate makes every history of it periods.
    fn of(histories: &[TemporalHistory]) -> MergedForm {
        if histories.iter().any(|history| matches!(history, TemporalHistory::Aggregates(_))) {
            MergedForm::Periods
        } else {
            MergedForm::Instances
        }
    }

    /// What a page of this form ends on at `instant`.
    const fn edge(self, instant: DateTime<Utc>) -> PageEdge {
        match self {
            MergedForm::Instances => PageEdge::Instant(instant),
            MergedForm::Periods => PageEdge::PeriodBoundary(instant),
        }
    }
}

/// The instant the merged history is complete up to, in the direction the
/// response is paged in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryBound {
    /// The interval the request asked for, which fixes the range-start and
    /// the window the continuation reads.
    interval: TimeInterval,
    /// The direction the rest of the history is paged in.
    direction: PageDirection,
    /// The tightest stop of every contributor.
    stop: Stop,
    /// The registrations of the sources whose exclusive stop is the bound,
    /// which are reported when the stop has to be taken inclusively.
    cut_short: Vec<Identifier>,
    /// The fewest instances the bound admits of any attribute stream of the
    /// merged history, counted by [`HistoryCompleteness::trim`].
    fewest_served: Option<u64>,
}

impl HistoryCompleteness {
    /// How complete the merge of the local read and what `sources` answered
    /// is.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Query`] when the request's temporal window
    /// cannot be read back, which validation has already ruled out.
    pub(crate) fn of(
        params: &TemporalQueryParameters,
        local: Option<TemporalRange>,
        sources: &[ForwardedTemporalEntities],
    ) -> Result<HistoryCompleteness, TemporalError> {
        if sources.iter().all(|source| source.entities.is_empty()) {
            return Ok(HistoryCompleteness::Local(local));
        }
        // Clause 6.3.10: "backwards" with `lastN`, "forwards" without it.
        let direction = params.last_n.map_or(PageDirection::Forward, |requested| PageDirection::Backward { requested });
        let local_stop = local.as_ref().and_then(|range| range.observed).map(|(least, most)| Stop {
            at: match direction {
                PageDirection::Forward => most,
                PageDirection::Backward { .. } => least,
            },
            side: StopSide::Inclusive,
        });
        let source_stops = sources.iter().flat_map(|source| {
            source.partial_ranges.iter().map(|range| Stop {
                at: range.end(),
                side: StopSide::Exclusive,
            })
        });
        let Some(stop) = local_stop.into_iter().chain(source_stops).reduce(|held, stop| held.tighter(stop, direction)) else {
            return Ok(HistoryCompleteness::Complete);
        };
        let cut_short = match stop.side {
            StopSide::Exclusive => sources
                .iter()
                .filter(|source| source.partial_ranges.iter().map(NgsiLdContentRange::end).any(|end| end == stop.at))
                .filter_map(|source| source.registration_id.clone()) // clone: the bound outlives the answers it was read from
                .collect(),
            StopSide::Inclusive => Vec::new(),
        };
        let interval = match local {
            Some(range) => range.interval,
            None => params.window()?.interval,
        };
        Ok(HistoryCompleteness::UpTo(HistoryBound {
            interval,
            direction,
            stop,
            cut_short,
            fewest_served: None,
        }))
    }

    /// Leave out of every merged entity the instances past the bound the
    /// history is complete up to, and return the clause 6.3.17 warnings of
    /// the sources whose stop had to be taken inclusively.
    ///
    /// The bound is settled here, against the histories it trims: an
    /// aggregated merge takes every stop inclusively, an exclusive stop that
    /// would leave no instance is taken inclusively, and the fewest instances
    /// it admits of any stream are counted before anything is left out.
    pub(crate) fn trim(&mut self, entities: &mut [TemporalHistory], timeproperty: TimeProp) -> Vec<NgsiLdWarning> {
        let HistoryCompleteness::UpTo(bound) = self else {
            return Vec::new();
        };
        let mut warnings = Vec::new();
        match (MergedForm::of(entities), bound.stop.side) {
            (MergedForm::Periods, StopSide::Exclusive) => bound.stop.side = StopSide::Inclusive,
            (MergedForm::Instances, StopSide::Exclusive) if !bound.admits_any(entities, timeproperty) => {
                bound.stop.side = StopSide::Inclusive;
                warnings.extend(bound.cut_short.iter().map(history_cut_short));
            }
            (MergedForm::Instances, StopSide::Exclusive) | (MergedForm::Instances | MergedForm::Periods, StopSide::Inclusive) => {}
        }
        bound.fewest_served = entities
            .iter()
            .filter_map(|entity| entity.fewest_stream_instances(timeproperty, |instant| bound.admits(instant)))
            .min()
            .map(|fewest| u64::try_from(fewest).unwrap_or(u64::MAX));
        for entity in entities {
            entity.retain_instants(timeproperty, |instant| bound.admits(instant));
        }
        warnings
    }

    /// The range the response reports for the `page` it answers with, and
    /// the request that continues it, or `None` when it answers with the
    /// whole history.
    pub(crate) fn range(self, page: &[TemporalHistory], timeproperty: TimeProp) -> Option<TemporalRange> {
        match self {
            HistoryCompleteness::Local(range) => range,
            HistoryCompleteness::Complete => None,
            HistoryCompleteness::UpTo(bound) => {
                let observed = instant_span(page, timeproperty);
                let edge = observed.map(|(least, most)| match bound.direction {
                    PageDirection::Forward => most,
                    PageDirection::Backward { .. } => least,
                });
                let next = edge
                    .and_then(|edge| TemporalContinuation::following(bound.interval, bound.direction, MergedForm::of(page).edge(edge), bound.fewest_served));
                Some(TemporalRange {
                    interval: bound.interval,
                    observed,
                    direction: bound.direction,
                    next,
                })
            }
        }
    }
}

/// The least and most recent instants any instance of any of `histories`
/// carries on `timeproperty`.
fn instant_span(histories: &[TemporalHistory], timeproperty: TimeProp) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    histories
        .iter()
        .filter_map(|history| history.instant_span(timeproperty))
        .fold(None, |span, (least, most)| {
            Some(span.map_or((least, most), |(held_least, held_most): (DateTime<Utc>, DateTime<Utc>)| {
                (held_least.min(least), held_most.max(most))
            }))
        })
}

impl HistoryBound {
    /// Whether an instance at `instant` lies on the complete side of the bound.
    fn admits(&self, instant: DateTime<Utc>) -> bool {
        match (self.direction, self.stop.side) {
            (PageDirection::Forward, StopSide::Inclusive) => instant <= self.stop.at,
            (PageDirection::Forward, StopSide::Exclusive) => instant < self.stop.at,
            (PageDirection::Backward { .. }, StopSide::Inclusive) => instant >= self.stop.at,
            (PageDirection::Backward { .. }, StopSide::Exclusive) => instant > self.stop.at,
        }
    }

    /// Whether the bound admits any instance of `entities`.
    fn admits_any(&self, entities: &[TemporalHistory], timeproperty: TimeProp) -> bool {
        instant_span(entities, timeproperty).is_some_and(|(least, most)| match self.direction {
            PageDirection::Forward => self.admits(least),
            PageDirection::Backward { .. } => self.admits(most),
        })
    }
}

#[cfg(test)]
mod tests;
