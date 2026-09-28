//! The one boundary a partial temporal response is cut at (clause 6.3.10).
//!
//! Every attribute stream of a temporal read is paged on its own: the store
//! keeps the first page of each and reports how many instances each holds in
//! the window. One response carries one `Content-Range`, though, and a client
//! resumes the whole response from its `range-end`. If attribute A was cut at
//! 01:19 while attribute B ran on to 01:22, a client resuming forwards from
//! 01:22 would never see A's instances between the two. So every stream of every
//! entity in the response is cut at the same instant: forwards at the earliest
//! instant any cut stream stopped at, backwards at the latest instant any cut
//! stream started from. A stream the common cut leaves empty stays in the
//! response as an attribute with no instances, which is what the official test
//! suite reads (ETSI TP `020_14` and `021_15`).

use crate::temporal::entity::is_scope_attribute;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time_prop::TimeProp, uri::Uri};
use andromeda_ngsi_ld_model::temporal::TemporalHistoryRow;
use andromeda_ngsi_ld_query::temporal::instance_page::PageDirection;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// The instant every stream of a partial response is cut at, and the side of
/// it that is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageCut {
    /// Whether the page runs forwards or backwards.
    pub direction: PageDirection,
    /// The last instant a forward page keeps, or the first instant a backward
    /// page keeps.
    pub boundary: DateTime<Utc>,
}

impl PageCut {
    /// The common cut of a response made of several entities' pages, or `None`
    /// when no stream of any of them was cut.
    ///
    /// Each entity contributes the cut its own page reading implies; forwards
    /// the earliest of them wins and backwards the latest, which is what keeps
    /// every instance the response leaves out beyond its `range-end`.
    #[must_use]
    pub fn common(cuts: impl IntoIterator<Item = PageCut>) -> Option<PageCut> {
        cuts.into_iter().reduce(|left, right| match left.direction {
            PageDirection::Forward => {
                if right.boundary < left.boundary {
                    right
                } else {
                    left
                }
            }
            PageDirection::Backward { .. } => {
                if right.boundary > left.boundary {
                    right
                } else {
                    left
                }
            }
        })
    }

    /// The cut one entity's page reading implies, or `None` when the page left
    /// no instance of any stream out.
    ///
    /// A stream was cut when the store reported more instances in the window
    /// than the page kept of it. Scope history carries no position on the
    /// requested axis (clause 4.18 makes a Scope valid over a span rather than
    /// at an instant), so it takes no part in the cut.
    #[must_use]
    pub fn of_page(rows: &[TemporalHistoryRow], timeproperty: TimeProp, direction: PageDirection) -> Option<PageCut> {
        let mut streams: HashMap<StreamKey<'_>, StreamPage> = HashMap::new();
        for row in rows.iter().filter(|row| !is_scope_attribute(&row.attribute_name)) {
            let stream = streams.entry((&row.attribute_name, row.dataset_id.as_ref())).or_default();
            stream.kept += 1;
            stream.total = stream.total.max(row.partition_total.unwrap_or(0));
            if let Some(instant) = row.time_axis(timeproperty) {
                stream.first = Some(stream.first.map_or(instant, |first| first.min(instant)));
                stream.last = Some(stream.last.map_or(instant, |last| last.max(instant)));
            }
        }
        let edges = streams.into_values().filter(StreamPage::was_cut).filter_map(|stream| match direction {
            PageDirection::Forward => stream.last,
            PageDirection::Backward { .. } => stream.first,
        });
        let boundary = match direction {
            PageDirection::Forward => edges.min(),
            PageDirection::Backward { .. } => edges.max(),
        }?;
        Some(PageCut { direction, boundary })
    }

    /// Whether a row stays in the response under this cut.
    ///
    /// Scope history is kept whatever its instant; a row carrying no instant on
    /// the requested axis has no position relative to the cut and is left out.
    #[must_use]
    pub fn keeps(&self, row: &TemporalHistoryRow, timeproperty: TimeProp) -> bool {
        if is_scope_attribute(&row.attribute_name) {
            return true;
        }
        row.time_axis(timeproperty).is_some_and(|instant| match self.direction {
            PageDirection::Forward => instant <= self.boundary,
            PageDirection::Backward { .. } => instant >= self.boundary,
        })
    }
}

/// One attribute stream: an attribute and the `datasetId` of its instances.
type StreamKey<'a> = (&'a AttributeName, Option<&'a Uri>);

/// What one stream's page holds.
#[derive(Debug, Default)]
struct StreamPage {
    /// Instances the page kept.
    kept: i64,
    /// Instances the stream holds in the window, as the store reported them.
    total: i64,
    /// The least recent instant kept.
    first: Option<DateTime<Utc>>,
    /// The most recent instant kept.
    last: Option<DateTime<Utc>>,
}

impl StreamPage {
    /// Whether the page left an instance of the stream out.
    const fn was_cut(&self) -> bool {
        self.total > self.kept
    }
}

/// The most instances of one stream that share one instant on the requested
/// axis, among the rows a page kept.
///
/// A page never splits such a group, so this is how far a page can overshoot
/// its size; the service refuses a group larger than the broker's maximum
/// rather than render it.
#[must_use]
pub fn largest_simultaneous_group<'a>(rows: impl IntoIterator<Item = &'a TemporalHistoryRow>, timeproperty: TimeProp) -> usize {
    let mut groups: HashMap<(StreamKey<'a>, DateTime<Utc>), usize> = HashMap::new();
    for row in rows.into_iter().filter(|row| !is_scope_attribute(&row.attribute_name)) {
        if let Some(instant) = row.time_axis(timeproperty) {
            *groups.entry(((&row.attribute_name, row.dataset_id.as_ref()), instant)).or_default() += 1;
        }
    }
    groups.into_values().max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use crate::temporal::page_cut::{PageCut, largest_simultaneous_group};
    use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, instance_identity::InstanceIdentity, time_prop::TimeProp};
    use andromeda_ngsi_ld_model::temporal::{TemporalHistoryRow, TemporalValue};
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_ngsi_ld_query::temporal::instance_page::PageDirection;
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use chrono::{DateTime, Duration, TimeZone, Utc};
    use uuid::Uuid;

    fn minute(offset: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2020, 1, 1, 1, 0, 0).single().expect("valid instant") + Duration::minutes(offset)
    }

    /// One row of `attribute` observed at `minute(offset)` in a stream whose
    /// window holds `total` instances.
    fn row(attribute: &str, offset: i64, total: i64) -> TemporalHistoryRow {
        TemporalHistoryRow {
            attribute_name: format!("https://example.com/{attribute}").parse().expect("valid attribute name"),
            attribute_type: AttributeType::Property,
            dataset_id: None,
            value: TemporalValue::Number(JsonNumber::from(offset)),
            observed_at: Some(minute(offset)),
            recorded_at: minute(offset),
            created_at: None,
            modified_at: None,
            instance: InstanceIdentity::Generated(Uuid::new_v4()),
            metadata: None,
            partition_total: Some(total),
        }
    }

    /// A stream holding the minutes `from..=to` of a window of `total`.
    fn stream(attribute: &str, from: i64, to: i64, total: i64) -> Vec<TemporalHistoryRow> {
        (from..=to).map(|offset| row(attribute, offset, total)).collect()
    }

    /// ETSI TP `020_13`: `speed` holds 01:01-01:20 and `fuelLevel` 01:03-01:22,
    /// 20 of each; a forward page of 19 keeps 01:01-01:19 and 01:03-01:21.
    /// Both were cut, so the response ends at the earlier of their last
    /// instants, 01:19.
    #[test]
    fn a_forward_cut_is_the_earliest_last_instant_of_any_cut_stream() {
        let mut rows = stream("speed", 1, 19, 20);
        rows.extend(stream("fuelLevel", 3, 21, 20));
        let cut = PageCut::of_page(&rows, TimeProp::Observed, PageDirection::Forward).expect("both streams were cut");
        pretty_assertions::assert_eq!(cut.boundary, minute(19));
        assert!(
            rows.iter()
                .filter(|row| cut.keeps(row, TimeProp::Observed))
                .all(|row| row.observed_at <= Some(minute(19)))
        );
    }

    /// Backwards the response starts at the latest first instant of any cut
    /// stream: `speed` kept 01:02-01:20 and `fuelLevel` 01:04-01:22, so both
    /// are cut back to 01:04.
    #[test]
    fn a_backward_cut_is_the_latest_first_instant_of_any_cut_stream() {
        let mut rows = stream("speed", 2, 20, 20);
        rows.extend(stream("fuelLevel", 4, 22, 20));
        let direction = PageDirection::Backward { requested: 20 };
        let cut = PageCut::of_page(&rows, TimeProp::Observed, direction).expect("both streams were cut");
        pretty_assertions::assert_eq!(cut.boundary, minute(4));
        let kept = rows.iter().filter(|row| cut.keeps(row, TimeProp::Observed)).count();
        pretty_assertions::assert_eq!(kept, 17 + 19);
    }

    /// A stream the page kept whole imposes nothing, and a page that kept every
    /// stream whole is not partial at all.
    #[test]
    fn a_page_that_cut_no_stream_has_no_cut() {
        let mut rows = stream("speed", 1, 5, 5);
        rows.extend(stream("fuelLevel", 3, 9, 7));
        assert!(PageCut::of_page(&rows, TimeProp::Observed, PageDirection::Forward).is_none());
    }

    /// A stream that was not cut is still filtered to the common boundary, and
    /// one left with nothing is simply empty.
    #[test]
    fn an_uncut_stream_is_filtered_to_the_boundary_of_a_cut_one() {
        let mut rows = stream("speed", 1, 5, 50);
        rows.extend(stream("fuelLevel", 30, 32, 3));
        let cut = PageCut::of_page(&rows, TimeProp::Observed, PageDirection::Forward).expect("speed was cut");
        pretty_assertions::assert_eq!(cut.boundary, minute(5));
        assert!(
            rows.iter()
                .filter(|row| row.attribute_name.as_str().ends_with("fuelLevel"))
                .all(|row| !cut.keeps(row, TimeProp::Observed))
        );
    }

    /// Several entities of one query page share the cut: forwards the earliest
    /// boundary, backwards the latest.
    #[test]
    fn the_common_cut_of_several_entities_is_the_most_restrictive_one() {
        let forward = |offset| PageCut {
            direction: PageDirection::Forward,
            boundary: minute(offset),
        };
        pretty_assertions::assert_eq!(PageCut::common([forward(19), forward(7), forward(30)]), Some(forward(7)));
        let backward = |offset| PageCut {
            direction: PageDirection::Backward { requested: 50 },
            boundary: minute(offset),
        };
        pretty_assertions::assert_eq!(PageCut::common([backward(4), backward(9), backward(1)]), Some(backward(9)));
        pretty_assertions::assert_eq!(PageCut::common([]), None);
    }

    /// Scope history has no position on the axis and neither decides nor
    /// suffers the cut.
    #[test]
    fn scope_history_takes_no_part_in_the_cut() {
        let mut scope = row("unused", 50, 99);
        scope.attribute_name = expanded_properties::SCOPE.as_str().parse().expect("the scope IRI");
        let mut rows = stream("speed", 1, 5, 50);
        // clone: the fixture row is both part of the page and asserted on.
        rows.push(scope.clone());
        let cut = PageCut::of_page(&rows, TimeProp::Observed, PageDirection::Forward).expect("speed was cut");
        pretty_assertions::assert_eq!(cut.boundary, minute(5));
        assert!(cut.keeps(&scope, TimeProp::Observed));
    }

    /// Instances sharing one instant form one group, counted per stream.
    #[test]
    fn the_largest_group_counts_instances_sharing_one_instant_in_one_stream() {
        let mut rows = stream("speed", 1, 3, 3);
        rows.extend((0..4).map(|_| row("speed", 7, 7)));
        rows.extend((0..2).map(|_| row("fuelLevel", 7, 2)));
        pretty_assertions::assert_eq!(largest_simultaneous_group(&rows, TimeProp::Observed), 4);
        pretty_assertions::assert_eq!(largest_simultaneous_group(Vec::<TemporalHistoryRow>::new().iter(), TimeProp::Observed), 0);
    }
}
