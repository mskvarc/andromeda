//! The one whole-range period a split query's hub-computed aggregates report
//! (clause 4.5.19.1).
//!
//! The entities read locally and the ones rendered over merged instances are
//! two reads of one response, each closed to the widest instant of its own
//! rows. For a query leaving a side open each read ends at its own extreme
//! instant, so the response closes them again to the widest across both. An
//! aggregate a source answered keeps the period the source answered.

use crate::temporal::TemporalQueryParameters;
use andromeda_ngsi_ld_model::temporal::{aggregated_entity::AggregatedTemporalEntity, temporal_history::TemporalHistory};
use andromeda_ngsi_ld_render::temporal::whole_range::WholeRange;

/// Close the aggregates of `local` and `rendered` to the one range of the
/// response, when the query asks for a whole-range period.
pub(crate) fn close_hub_aggregates(params: &TemporalQueryParameters, local: &mut [TemporalHistory], rendered: &mut [TemporalHistory]) {
    if params.aggr_period_duration.is_some() {
        return;
    }
    let range = WholeRange::of_entities(aggregates(local).chain(aggregates(rendered)));
    for entity in aggregates_mut(local).chain(aggregates_mut(rendered)) {
        range.close_entity(entity);
    }
}

/// The aggregated entities of `histories`.
fn aggregates(histories: &[TemporalHistory]) -> impl Iterator<Item = &AggregatedTemporalEntity> {
    histories.iter().filter_map(|history| match history {
        TemporalHistory::Aggregates(entity) => Some(entity),
        TemporalHistory::Instances(_) => None,
    })
}

/// The aggregated entities of `histories`, mutably.
fn aggregates_mut(histories: &mut [TemporalHistory]) -> impl Iterator<Item = &mut AggregatedTemporalEntity> {
    histories.iter_mut().filter_map(|history| match history {
        TemporalHistory::Aggregates(entity) => Some(entity),
        TemporalHistory::Instances(_) => None,
    })
}

#[cfg(test)]
mod tests {
    use crate::temporal::query::whole_range_join::close_hub_aggregates;
    use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName, attribute_type::AttributeType};
    use andromeda_ngsi_ld_model::{
        entity_type::EntityTypeValue,
        temporal::{StreamExtent, TemporalAggregationRow, aggregated_entity::AggregatedTemporalEntity, temporal_history::TemporalHistory},
    };
    use andromeda_ngsi_ld_query::temporal::aggregation_period::AggregationPeriod;
    use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
    use chrono::{DateTime, Utc};

    fn at(raw: &str) -> DateTime<Utc> {
        raw.parse().expect("a valid instant")
    }

    /// An aggregated entity whose one whole-range period runs `start` to `end`.
    fn aggregated(id: &str, start: &str, end: &str) -> TemporalHistory {
        let row = TemporalAggregationRow {
            attribute_name: "https://example.com/speed".parse().expect("valid attribute name"),
            attribute_type: AttributeType::Property,
            dataset_id: None,
            bucket_start: Some(at(start)),
            bucket_end: Some(at(end)),
            avg: None,
            min: None,
            max: None,
            sum: None,
            sumsq: None,
            stddev: None,
            total_count: Some(1),
            distinct_count: None,
            group_keys: Vec::new(),
            stream_extent: StreamExtent::Whole,
        };
        TemporalHistory::Aggregates(AggregatedTemporalEntity::from_rows(
            id.parse().expect("an identifier"),
            EntityTypeValue::Single("Vehicle".parse().expect("a type")),
            &[row],
            &[AggrMethod::TotalCount],
            0,
            AttributeName::clone,
        ))
    }

    fn span(history: &TemporalHistory) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        match history {
            TemporalHistory::Aggregates(entity) => entity.period_span(),
            TemporalHistory::Instances(_) => None,
        }
    }

    /// Clause 4.5.19.1: the range belongs to the query, so a local entity
    /// ending at 10:00 and a rendered one ending at 11:00 report one range.
    #[test]
    fn local_and_rendered_aggregates_close_to_the_widest_instant() {
        let mut local = vec![aggregated("urn:ngsi-ld:Vehicle:1", "2024-01-01T00:00:00Z", "2024-01-01T10:00:00Z")];
        let mut rendered = vec![aggregated("urn:ngsi-ld:Vehicle:2", "2024-01-01T01:00:00Z", "2024-01-01T11:00:00Z")];
        close_hub_aggregates(&TemporalQueryParameters::default(), &mut local, &mut rendered);
        let widest = Some((at("2024-01-01T00:00:00Z"), at("2024-01-01T11:00:00Z")));
        pretty_assertions::assert_eq!(span(&local[0]), widest);
        pretty_assertions::assert_eq!(span(&rendered[0]), widest);
    }

    /// A query naming a period leaves every period as the store cut it.
    #[test]
    fn a_named_period_is_left_alone() {
        let mut local = vec![aggregated("urn:ngsi-ld:Vehicle:1", "2024-01-01T00:00:00Z", "2024-01-01T10:00:00Z")];
        let mut rendered = vec![aggregated("urn:ngsi-ld:Vehicle:2", "2024-01-01T01:00:00Z", "2024-01-01T11:00:00Z")];
        let params = TemporalQueryParameters {
            aggr_period_duration: AggregationPeriod::parse("PT1H").expect("a period"),
            ..TemporalQueryParameters::default()
        };
        close_hub_aggregates(&params, &mut local, &mut rendered);
        pretty_assertions::assert_eq!(span(&local[0]), Some((at("2024-01-01T00:00:00Z"), at("2024-01-01T10:00:00Z"))));
        pretty_assertions::assert_eq!(span(&rendered[0]), Some((at("2024-01-01T01:00:00Z"), at("2024-01-01T11:00:00Z"))));
    }

    /// With nothing rendered the local aggregates already agree.
    #[test]
    fn local_aggregates_alone_are_unchanged() {
        let mut local = vec![aggregated("urn:ngsi-ld:Vehicle:1", "2024-01-01T00:00:00Z", "2024-01-01T10:00:00Z")];
        close_hub_aggregates(&TemporalQueryParameters::default(), &mut local, &mut []);
        pretty_assertions::assert_eq!(span(&local[0]), Some((at("2024-01-01T00:00:00Z"), at("2024-01-01T10:00:00Z"))));
    }
}
