//! Choosing the instances of every attribute stream one temporal page keeps,
//! for a reader that assembles history in memory.
//!
//! The PostgreSQL reader narrows each stream in its statement; the pending
//! overlay unions stored history with intents still in flight and has to narrow
//! the union itself. Both apply the same rule, which is stated here once for the
//! in-memory side: a `lastN` answered in full keeps exactly the last `n`
//! instances, and a clause 6.3.10 page keeps the first `size` instances in its
//! direction together with every instance sharing the timestamp of the last of
//! them. A forward page does not count the instances sitting on the inclusive
//! lower bound of its window, which are the group the previous page ended on.
//! Every kept row carries its stream's total in the window, which is how the
//! page later tells whether it left instances out.

use crate::port::history_reader::TemporalHistoryObservation;
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_ngsi_ld_query::temporal::{compiled::CompiledTemporalQuery, instance_page::InstancePage, window::TimeInterval};
use chrono::{DateTime, Utc};
use std::cmp::Reverse;

/// Narrow `observations`, grouped by stream (attribute and `datasetId`) and
/// ascending on the requested axis within each, to the page `compiled` asks for.
pub(crate) fn select_page(observations: Vec<TemporalHistoryObservation>, compiled: &CompiledTemporalQuery) -> Vec<TemporalHistoryObservation> {
    let timeproperty = compiled.window.timeproperty;
    let mut selected = Vec::with_capacity(observations.len());
    let mut remaining = observations.into_iter().peekable();
    while let Some(first) = remaining.next() {
        let mut stream = vec![first];
        while let Some(next) =
            remaining.next_if(|next| next.row.attribute_name == stream[0].row.attribute_name && next.row.dataset_id == stream[0].row.dataset_id)
        {
            stream.push(next);
        }
        selected.extend(select_stream(stream, compiled.instances.page, compiled.window.interval, timeproperty));
    }
    selected
}

/// Narrow one stream to its page.
fn select_stream(
    mut stream: Vec<TemporalHistoryObservation>,
    page: InstancePage,
    interval: TimeInterval,
    timeproperty: TimeProp,
) -> Vec<TemporalHistoryObservation> {
    let total = i64::try_from(stream.len()).unwrap_or(i64::MAX);
    for observation in &mut stream {
        observation.row.partition_total = Some(total);
    }
    let axis = |observation: &TemporalHistoryObservation| observation.row.time_axis(timeproperty);
    match page {
        InstancePage::Whole => stream,
        InstancePage::LastN { n } => {
            let keep = usize::try_from(n).unwrap_or(usize::MAX);
            let skip = stream.len().saturating_sub(keep);
            stream.into_iter().skip(skip).collect()
        }
        InstancePage::Forward { size } => {
            // Ascending, an instance with no instant on the axis sorts last.
            let mut keys: Vec<(bool, Option<DateTime<Utc>>)> = stream.iter().map(|observation| forward_key(axis(observation))).collect();
            keys.sort_unstable();
            let lower = match interval {
                TimeInterval::After(lower) | TimeInterval::Between { from: lower, .. } => Some(lower),
                TimeInterval::Unbounded | TimeInterval::Before(_) => None,
            };
            let uncounted = lower.map_or(0, |lower| stream.iter().filter(|observation| axis(observation) == Some(lower)).count());
            stream
                .into_iter()
                .filter(|observation| {
                    let rank = 1 + keys.partition_point(|key| *key < forward_key(axis(observation)));
                    rank.saturating_sub(uncounted) <= usize::try_from(size).unwrap_or(usize::MAX)
                })
                .collect()
        }
        InstancePage::Backward { size, .. } => {
            // Descending, an instance with no instant on the axis sorts last.
            let mut keys: Vec<(bool, Reverse<Option<DateTime<Utc>>>)> = stream.iter().map(|observation| backward_key(axis(observation))).collect();
            keys.sort_unstable();
            stream
                .into_iter()
                .filter(|observation| {
                    let rank = 1 + keys.partition_point(|key| *key < backward_key(axis(observation)));
                    rank <= usize::try_from(size).unwrap_or(usize::MAX)
                })
                .collect()
        }
    }
}

/// The ascending rank key of an instant, an absent one ordering after all.
const fn forward_key(instant: Option<DateTime<Utc>>) -> (bool, Option<DateTime<Utc>>) {
    (instant.is_none(), instant)
}

/// The descending rank key of an instant, an absent one ordering after all.
const fn backward_key(instant: Option<DateTime<Utc>>) -> (bool, Reverse<Option<DateTime<Utc>>>) {
    (instant.is_none(), Reverse(instant))
}

#[cfg(test)]
mod tests {
    use crate::{instance_page_selection::select_page, port::history_reader::TemporalHistoryObservation};
    use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, instance_identity::InstanceIdentity, temporal_format::TemporalFormat, time_prop::TimeProp};
    use andromeda_ngsi_ld_model::temporal::{TemporalHistoryRow, TemporalValue};
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        temporal::{
            compiled::CompiledTemporalQuery,
            instance_page::InstancePage,
            instance_selection::InstanceSelection,
            representation::Representation,
            window::{TemporalWindow, TimeInterval},
        },
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use chrono::{DateTime, Duration, TimeZone, Utc};
    use uuid::Uuid;

    fn minute(offset: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).single().expect("valid instant") + Duration::minutes(offset)
    }

    fn observation(offset: i64) -> TemporalHistoryObservation {
        TemporalHistoryObservation {
            row: TemporalHistoryRow {
                attribute_name: "https://example.com/speed".parse().expect("valid attribute name"),
                attribute_type: AttributeType::Property,
                dataset_id: None,
                value: TemporalValue::Number(JsonNumber::from(offset)),
                observed_at: Some(minute(offset)),
                recorded_at: minute(offset),
                created_at: None,
                modified_at: None,
                instance: InstanceIdentity::Generated(Uuid::new_v4()),
                metadata: None,
                partition_total: None,
            },
            identity: None,
        }
    }

    fn compiled(page: InstancePage, interval: TimeInterval) -> CompiledTemporalQuery {
        CompiledTemporalQuery::builder()
            .entities(EntitySelection::builder().build())
            .window(TemporalWindow::builder().timeproperty(TimeProp::Observed).interval(interval).build())
            .instances(InstanceSelection::builder().page(page).build())
            .representation(
                Representation::builder()
                    .format(TemporalFormat::Normalized)
                    .sys_attrs(SysAttrsInclusion::Strip)
                    .build(),
            )
            .build()
    }

    fn minutes_of(selected: &[TemporalHistoryObservation]) -> Vec<i64> {
        selected
            .iter()
            .filter_map(|observation| observation.row.observed_at)
            .map(|instant| (instant - minute(0)).num_minutes())
            .collect()
    }

    /// Minutes 0 to 5, with a group of three at minute 2.
    fn stream() -> Vec<TemporalHistoryObservation> {
        [0, 1, 2, 2, 2, 3, 4, 5].into_iter().map(observation).collect()
    }

    /// A forward page of three keeps the three least recent instances and the
    /// whole group the third belongs to.
    #[test]
    fn a_forward_page_keeps_the_group_at_its_edge_whole() {
        let selected = select_page(stream(), &compiled(InstancePage::Forward { size: 3 }, TimeInterval::Unbounded));
        pretty_assertions::assert_eq!(minutes_of(&selected), vec![0, 1, 2, 2, 2]);
        assert!(selected.iter().all(|observation| observation.row.partition_total == Some(8)));
    }

    /// The instances on the inclusive lower bound of `after` are not counted,
    /// so a resumed page advances past them.
    #[test]
    fn a_forward_page_does_not_count_the_instances_on_its_lower_bound() {
        let resumed: Vec<TemporalHistoryObservation> = stream()
            .into_iter()
            .filter(|observation| observation.row.observed_at >= Some(minute(2)))
            .collect();
        let selected = select_page(resumed, &compiled(InstancePage::Forward { size: 2 }, TimeInterval::After(minute(2))));
        pretty_assertions::assert_eq!(minutes_of(&selected), vec![2, 2, 2, 3, 4]);
    }

    /// A backward page keeps the most recent instances and the whole group at
    /// its lower edge.
    #[test]
    fn a_backward_page_keeps_the_group_at_its_lower_edge_whole() {
        let selected = select_page(stream(), &compiled(InstancePage::Backward { size: 4, requested: 50 }, TimeInterval::Unbounded));
        pretty_assertions::assert_eq!(minutes_of(&selected), vec![2, 2, 2, 3, 4, 5]);
    }

    /// A `lastN` answered in full keeps exactly `n` instances.
    #[test]
    fn a_last_n_keeps_exactly_n_instances() {
        let selected = select_page(stream(), &compiled(InstancePage::LastN { n: 4 }, TimeInterval::Unbounded));
        pretty_assertions::assert_eq!(minutes_of(&selected), vec![2, 3, 4, 5]);
    }

    /// Without a page every instance stays.
    #[test]
    fn the_whole_history_keeps_every_instance() {
        pretty_assertions::assert_eq!(select_page(stream(), &compiled(InstancePage::Whole, TimeInterval::Unbounded)).len(), 8);
    }
}
