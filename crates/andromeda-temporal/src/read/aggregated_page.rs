//! Reading the aggregated representation of the entities of one response and
//! cutting it at one period boundary (clauses 4.5.19 and 6.3.10).
//!
//! Every attribute stream is paged by whole periods on its own, and one
//! response carries one `Content-Range`, so every entity of a response is read
//! before any is rendered: the cut is the earliest period boundary any stream
//! of any of them was cut at, and the whole-range period an absent
//! `aggrPeriodDuration` asks for reads the same for every stream of the
//! response (clause 4.5.19.1).

use crate::{
    error::TemporalError,
    grouping_depth::grouping_depth,
    port::read_scope::TemporalReadScope,
    read::rendered_entity::{HistoryPresence, RenderedTemporalEntity, presence_of},
    result_bound::ResultBound,
    service::TemporalService,
};
use andromeda_ngsi_ld_compaction::{aggregated_entity::aggregated_entity, name_compaction::NameCompaction};
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::temporal::{TemporalAggregationRow, temporal_history::TemporalHistory};
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_ngsi_ld_render::temporal::{page_cut::PageCut, period_cut::period_time_range, whole_range::WholeRange};

/// One entity's aggregated periods as its page read them, before the
/// response's common cut is applied.
#[derive(Debug)]
pub(crate) struct EntityAggregationPage {
    /// The entity the periods belong to.
    pub(crate) entity_id: Identifier,
    /// The periods the page kept of every stream.
    pub(crate) rows: Vec<TemporalAggregationRow>,
    /// Whether the window selected any history for the entity, judged before
    /// the cut: an entity the cut emptied continues on the next page.
    presence: HistoryPresence,
}

impl EntityAggregationPage {
    /// Whether the window selected any history for the entity at all.
    pub(crate) const fn presence(&self) -> HistoryPresence {
        self.presence
    }
}

impl TemporalService {
    /// Read the page of one entity's aggregated periods the compiled query
    /// selects.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the aggregation cannot be read or a
    /// group spans inputs a requested method cannot.
    pub(crate) async fn read_aggregation_page(
        &self,
        scope: &TemporalReadScope<'_>,
        entity_id: &Identifier,
        compiled: &CompiledTemporalQuery,
    ) -> Result<EntityAggregationPage, TemporalError> {
        let rows = self.reader.query_entity_aggregation(scope, entity_id, compiled).await?;
        let presence = presence_of(rows.is_empty());
        Ok(EntityAggregationPage {
            entity_id: entity_id.clone(), // clone: the page owns the identifier it is rendered under.
            rows,
            presence,
        })
    }
}

/// Cut every page of a response at the one period boundary any stream of any
/// of them was cut at, and report the whole-range period every stream of the
/// response shares; the cut, or `None` when the response is complete.
pub(crate) fn settle_aggregation_pages(compiled: &CompiledTemporalQuery, pages: &mut [EntityAggregationPage]) -> Option<PageCut> {
    let cut = compiled
        .instances
        .page
        .direction()
        .and_then(|direction| PageCut::common(pages.iter().filter_map(|page| PageCut::of_periods(&page.rows, direction))));
    if let Some(cut) = cut {
        for page in pages.iter_mut() {
            page.rows.retain(|row| cut.keeps_period(row));
        }
    }
    if compiled.representation.aggr_period_duration.is_none() {
        let range = WholeRange::of_response(pages.iter().flat_map(|page| page.rows.iter()));
        for row in pages.iter_mut().flat_map(|page| page.rows.iter_mut()) {
            range.close(row);
        }
    }
    cut
}

/// The bounds a rendered aggregated page is held to.
#[derive(Clone, Copy)]
pub(crate) struct AggregationRendering<'r> {
    /// The query the page was read under.
    pub(crate) compiled: &'r CompiledTemporalQuery,
    /// The names the request context gives the attributes and types.
    pub(crate) names: &'r NameCompaction,
    /// The distinct groups one grouped aggregation may produce at most.
    pub(crate) max_groups: u64,
    /// The response's common cut.
    pub(crate) cut: Option<PageCut>,
}

/// Build one entity's aggregated representation at the response's common
/// cut.
///
/// # Errors
///
/// Returns [`TemporalError::TooManyResults`] when a grouped aggregation
/// produced more distinct groups than the broker's bound: one per row,
/// mirroring the delete-by-query bound.
pub(crate) fn render_aggregation_page(
    page: EntityAggregationPage,
    entity_types: &[EntityTypeName],
    rendering: AggregationRendering<'_>,
) -> Result<RenderedTemporalEntity, TemporalError> {
    let compiled = rendering.compiled;
    if compiled.representation.format.is_grouped() {
        let groups = page.rows.len() as u64;
        if groups > rendering.max_groups {
            return Err(TemporalError::too_many_results(groups, rendering.max_groups, ResultBound::AggregationGroups));
        }
    }
    let presence = page.presence();
    let time_range = period_time_range(&page.rows);
    let EntityAggregationPage { entity_id, rows, .. } = page;
    let aggregates = aggregated_entity(
        entity_id,
        entity_types,
        &rows,
        &compiled.representation.aggr_methods,
        grouping_depth(compiled),
        rendering.names,
    );
    Ok(RenderedTemporalEntity {
        history: TemporalHistory::Aggregates(aggregates),
        presence,
        cut: rendering.cut,
        time_range,
    })
}

#[cfg(test)]
mod tests {
    use crate::read::{
        aggregated_page::{EntityAggregationPage, settle_aggregation_pages},
        rendered_entity::HistoryPresence,
    };
    use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_type::AttributeType, temporal_format::TemporalFormat, time_prop::TimeProp};
    use andromeda_ngsi_ld_model::temporal::{StreamExtent, TemporalAggregationRow};
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        temporal::{
            aggregation_period::AggregationPeriod,
            compiled::CompiledTemporalQuery,
            instance_page::InstancePage,
            instance_selection::InstanceSelection,
            period_budget::PeriodBudget,
            representation::Representation,
            window::{TemporalWindow, TimeInterval},
        },
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use chrono::{DateTime, Duration, Utc};
    use std::num::NonZeroU64;

    fn at(raw: &str) -> DateTime<Utc> {
        raw.parse().expect("a valid instant")
    }

    /// `totalCount` over `period` (none for the whole range) in `interval`,
    /// paged by 19 periods.
    fn aggregated(period: &str, interval: TimeInterval) -> CompiledTemporalQuery {
        let period = AggregationPeriod::parse(period).expect("a valid period");
        let page = match period {
            Some(_) => InstancePage::Periods {
                budget: PeriodBudget {
                    periods: 19,
                    instances: NonZeroU64::new(1_000_000).expect("a positive budget"),
                },
            },
            None => InstancePage::Whole,
        };
        CompiledTemporalQuery::builder()
            .entities(EntitySelection::builder().build())
            .window(TemporalWindow::builder().timeproperty(TimeProp::Observed).interval(interval).build())
            .instances(InstanceSelection::builder().page(page).build())
            .representation(
                Representation::builder()
                    .format(TemporalFormat::AggregatedValues)
                    .aggr_methods(vec![AggrMethod::TotalCount])
                    .aggr_period_duration(period)
                    .sys_attrs(SysAttrsInclusion::Strip)
                    .build(),
            )
            .build()
    }

    /// One period of `speed` from `start` to `end`, in a stream read to
    /// `extent`.
    fn period(start: DateTime<Utc>, end: DateTime<Utc>, extent: StreamExtent) -> TemporalAggregationRow {
        TemporalAggregationRow {
            attribute_name: "https://example.com/speed".parse().expect("valid attribute name"),
            attribute_type: AttributeType::Property,
            dataset_id: None,
            bucket_start: Some(start),
            bucket_end: Some(end),
            avg: None,
            min: None,
            max: None,
            sum: None,
            sumsq: None,
            stddev: None,
            total_count: Some(1),
            distinct_count: None,
            group_keys: Vec::new(),
            stream_extent: extent,
        }
    }

    /// The hourly periods `from..to` hours after 2024-01-01, in a stream read
    /// to `extent`.
    fn hours(from: i64, to: i64, extent: StreamExtent) -> Vec<TemporalAggregationRow> {
        let origin = at("2024-01-01T00:00:00Z");
        (from..to)
            .map(|hour| period(origin + Duration::hours(hour), origin + Duration::hours(hour + 1), extent))
            .collect()
    }

    fn page(entity: &str, rows: Vec<TemporalAggregationRow>) -> EntityAggregationPage {
        EntityAggregationPage {
            entity_id: entity.parse().expect("a valid identifier"),
            presence: HistoryPresence::Present,
            rows,
        }
    }

    /// Clause 6.3.10: one response, one `Content-Range`. Vehicle 1 was cut
    /// after 19 periods ending at 19:00; vehicle 2 holds 40 periods and was cut
    /// later, so both are cut at 19:00 and vehicle 2 keeps only the periods
    /// ending by then.
    #[test]
    fn every_entity_of_a_response_is_cut_at_the_earliest_period_boundary() {
        let compiled = aggregated("PT1H", TimeInterval::After(at("2024-01-01T00:00:00Z")));
        let mut pages = [
            page("urn:ngsi-ld:Vehicle:1", hours(0, 19, StreamExtent::Cut)),
            page("urn:ngsi-ld:Vehicle:2", hours(5, 24, StreamExtent::Cut)),
        ];
        let cut = settle_aggregation_pages(&compiled, &mut pages).expect("the response was cut");
        pretty_assertions::assert_eq!(cut.boundary, at("2024-01-01T19:00:00Z"));
        pretty_assertions::assert_eq!(pages[0].rows.len(), 19);
        pretty_assertions::assert_eq!(pages[1].rows.len(), 14);
        assert!(pages.iter().flat_map(|page| &page.rows).all(|row| row.bucket_end <= Some(cut.boundary)));
    }

    /// A response no stream of which was cut keeps every period.
    #[test]
    fn a_response_no_stream_was_cut_in_is_complete() {
        let compiled = aggregated("PT1H", TimeInterval::After(at("2024-01-01T00:00:00Z")));
        let mut pages = [page("urn:ngsi-ld:Vehicle:1", hours(0, 40, StreamExtent::Whole))];
        assert!(settle_aggregation_pages(&compiled, &mut pages).is_none());
        pretty_assertions::assert_eq!(pages[0].rows.len(), 40);
    }

    /// Clause 4.5.19.1: the whole-range period of `after` ends at the most
    /// recent instant any stream of the response holds, for every entity.
    #[test]
    fn the_whole_range_period_of_a_response_is_one_range_for_every_entity() {
        let compiled = aggregated("PT0S", TimeInterval::After(at("2024-01-01T00:00:00Z")));
        let start = at("2024-01-01T00:00:00Z");
        let mut pages = [
            page("urn:ngsi-ld:Vehicle:1", vec![period(start, at("2024-01-02T15:00:00Z"), StreamExtent::Whole)]),
            page("urn:ngsi-ld:Vehicle:2", vec![period(start, at("2024-01-01T09:00:00Z"), StreamExtent::Whole)]),
        ];
        assert!(settle_aggregation_pages(&compiled, &mut pages).is_none());
        for row in pages.iter().flat_map(|page| &page.rows) {
            pretty_assertions::assert_eq!(row.bucket_start, Some(start));
            pretty_assertions::assert_eq!(row.bucket_end, Some(at("2024-01-02T15:00:00Z")));
        }
    }
}
