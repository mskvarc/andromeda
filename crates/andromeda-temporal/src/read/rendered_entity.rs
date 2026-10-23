//! Reading the temporal evolution of one entity over a compiled window.
//!
//! Three callers need exactly this and nothing more: the query endpoint reads
//! it once per matching identifier, the clause 4.5.23 join reads it once per
//! Linked Entity it resolves, and both must produce the same history for the
//! same entity over the same window. Keeping the pipeline -- history read and
//! representation build -- in one place is what guarantees that. What is read
//! is the history in its expanded form (or, for an aggregated representation,
//! the aggregates); it is compacted under the request context only once every
//! holder's part has been gathered (clause 5.7.3).
//!
//! The window arrives already compiled rather than as request parameters, so a
//! caller that reads many entities compiles once instead of once per entity.

use crate::{
    error::TemporalError,
    port::read_scope::TemporalReadScope,
    read::{
        aggregated_page::{AggregationRendering, render_aggregation_page, settle_aggregation_pages},
        response_cut::ResponseCut,
    },
    result_bound::ResultBound,
    service::TemporalService,
    temporal_invariant::TemporalInvariant,
};
use andromeda_ngsi_ld_compaction::entity_compactor::EntityCompactor;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::temporal::{TemporalHistoryRow, temporal_history::TemporalHistory};
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_ngsi_ld_render::temporal::{
    entity::TemporalEntityRenderer,
    history_row::TimeRange,
    page_cut::{PageCut, largest_simultaneous_group},
};
use std::slice;

/// Name what an empty row set means, so the presence is read off a variant
/// rather than off a bare emptiness test at each use site.
pub(crate) const fn presence_of(is_empty: bool) -> HistoryPresence {
    if is_empty { HistoryPresence::Absent } else { HistoryPresence::Present }
}

/// Whether the compiled window actually selected any history for the entity.
///
/// The two callers disagree about what an empty window means, and neither can
/// tell from the representation alone — an entity with no history in range
/// renders as a well-formed temporal entity carrying no attribute instances.
/// A query drops it from the result set; a retrieval answers it with a 404 when
/// a window or an `attrs` selection was in force (clause 5.7.3.4). Reporting the
/// fact rather than deciding it keeps that policy with the endpoints that own
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryPresence {
    /// The window selected at least one observation.
    Present,
    /// The window selected nothing.
    Absent,
}

/// One entity's history, with the instants the caller needs in order to build
/// a `Content-Range`.
///
/// The range travels with the history because it is a property of *this*
/// reading: a caller aggregating many entities takes the widest range across
/// them, which it cannot do once the histories have been flattened into a
/// collection.
#[derive(Debug)]
pub(crate) struct RenderedTemporalEntity {
    /// The history the window selected.
    pub(crate) history: TemporalHistory,
    /// Whether the window selected any history at all.
    pub(crate) presence: HistoryPresence,
    /// The cut the history was made at, present when it is part of a partial
    /// response (clause 6.3.10).
    pub(crate) cut: Option<ResponseCut>,
    /// The earliest and latest instants the history actually carries.
    pub(crate) time_range: Option<TimeRange>,
}

/// One entity's history as its page read it, before the response's common cut
/// is applied.
#[derive(Debug)]
pub(crate) struct EntityHistoryPage {
    /// The entity the history belongs to.
    pub(crate) entity_id: Identifier,
    /// The rows the page kept of every stream.
    pub(crate) rows: Vec<TemporalHistoryRow>,
}

impl EntityHistoryPage {
    /// Whether the window selected any history for the entity at all.
    pub(crate) const fn presence(&self) -> HistoryPresence {
        presence_of(self.rows.is_empty())
    }
}

impl TemporalService {
    /// Read one entity's temporal evolution over an already-compiled window.
    ///
    /// The aggregated representation of clause 4.5.19 is read as the
    /// aggregates themselves, which carry the names of the request context
    /// already, because the aggregation keys are not IRIs; that path is taken
    /// here so both callers get the same treatment rather than one of them
    /// silently supporting fewer formats. Either representation is the
    /// entity's page, cut on its own (clause 6.3.10): a caller reading several
    /// entities into one response reads their pages and cuts them together
    /// instead.
    ///
    /// A window that selects nothing still reads -- into a temporal entity
    /// carrying no attribute instances -- and says so through
    /// [`RenderedTemporalEntity::presence`], because whether that is a result,
    /// an omission or a 404 is the caller's decision.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the history cannot be read, a page would
    /// carry more instances of one attribute sharing one instant than
    /// `temporal.max_edge_group`, or an aggregate's names cannot be compacted. Each caller decides
    /// whether that ends the operation or merely drops one entity from it.
    pub(crate) async fn render_one_temporal_entity(
        &self,
        scope: &TemporalReadScope<'_>,
        entity_id: &Identifier,
        entity_types: &[EntityTypeName],
        compiled: &CompiledTemporalQuery,
        compactor: &EntityCompactor<'_>,
    ) -> Result<RenderedTemporalEntity, TemporalError> {
        if compiled.representation.format.is_aggregated() {
            return self.render_aggregated_entity(scope, entity_id, entity_types, compiled, compactor).await;
        }
        let page = self.read_history_page(scope, entity_id, compiled).await?;
        let cut = self.common_page_cut(compiled, slice::from_ref(&page))?;
        render_history_page(page, entity_types, compiled, cut)
    }

    /// Read the page of one entity's history the compiled query selects.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Persistence`] when the history cannot be read.
    pub(crate) async fn read_history_page(
        &self,
        scope: &TemporalReadScope<'_>,
        entity_id: &Identifier,
        compiled: &CompiledTemporalQuery,
    ) -> Result<EntityHistoryPage, TemporalError> {
        let rows = self.reader.query_entity_history(scope, entity_id, compiled).await?.into_rows();
        Ok(EntityHistoryPage {
            entity_id: entity_id.clone(), // clone: the page owns the identifier it is rendered under.
            rows,
        })
    }

    /// The one cut every entity of a response is made at, or `None` when the
    /// response is complete (clause 6.3.10).
    ///
    /// A page carries the instances of one attribute sharing the instant at
    /// its edge whole, past its size if need be: a client resumes from a
    /// `range-end` `DateTime`, which cannot point inside an instant. The cut
    /// is counted over the pages before it drops anything, so a stream it
    /// empties still counts as served nothing.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::TooManyResults`] when a page would carry more
    /// instances of one attribute sharing one instant than
    /// `temporal.max_edge_group`: such a group is the one part of a page its
    /// size does not bound (clause 5.5.6 leaves the threshold to the
    /// implementation).
    pub(crate) fn common_page_cut(&self, compiled: &CompiledTemporalQuery, pages: &[EntityHistoryPage]) -> Result<Option<ResponseCut>, TemporalError> {
        let Some(direction) = compiled.instances.page.direction() else {
            return Ok(None);
        };
        let timeproperty = compiled.window.timeproperty;
        let cut = PageCut::common(pages.iter().filter_map(|page| PageCut::of_page(&page.rows, timeproperty, direction)));
        let bound = self.limits.instance_limits.edge_group;
        for page in pages {
            let kept = page.rows.iter().filter(|row| cut.is_none_or(|cut| cut.keeps(row, timeproperty)));
            let group = largest_simultaneous_group(kept, timeproperty);
            if group > usize::try_from(bound).unwrap_or(usize::MAX) {
                return Err(TemporalError::too_many_results(
                    u64::try_from(group).unwrap_or(u64::MAX),
                    u64::from(bound),
                    ResultBound::EdgeGroup,
                ));
            }
        }
        Ok(cut.map(|cut| ResponseCut::of_instances(cut, pages, timeproperty)))
    }

    /// Read the clause 4.5.19 aggregated representation of one entity, cut on
    /// its own at a period boundary (clause 6.3.10).
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the aggregation cannot be read, the
    /// grouped extension's group bound is exceeded, or a name cannot be
    /// compacted.
    async fn render_aggregated_entity(
        &self,
        scope: &TemporalReadScope<'_>,
        entity_id: &Identifier,
        entity_types: &[EntityTypeName],
        compiled: &CompiledTemporalQuery,
        compactor: &EntityCompactor<'_>,
    ) -> Result<RenderedTemporalEntity, TemporalError> {
        let mut pages = [self.read_aggregation_page(scope, entity_id, compiled).await?];
        let cut = settle_aggregation_pages(compiled, &mut pages);
        let [page] = pages;
        render_aggregation_page(
            page,
            entity_types,
            AggregationRendering {
                compiled,
                names: compactor.names().await?,
                max_groups: self.limits.aggregation_grouping_max_groups,
                cut,
            },
        )
    }
}

/// Build one entity's page at the response's common cut.
///
/// # Errors
///
/// Returns [`TemporalError`] when a stored row no longer reads as the instance
/// it recorded.
pub(crate) fn render_history_page(
    page: EntityHistoryPage,
    entity_types: &[EntityTypeName],
    compiled: &CompiledTemporalQuery,
    cut: Option<ResponseCut>,
) -> Result<RenderedTemporalEntity, TemporalError> {
    let presence = page.presence();
    let EntityHistoryPage { entity_id, rows } = page;
    let built = TemporalEntityRenderer::new(&entity_id, entity_types, rows, compiled, cut.map(|settled| settled.cut))
        .render()
        .map_err(|error| TemporalError::from(TemporalInvariant::RenderedInstance(error)))?;
    Ok(RenderedTemporalEntity {
        history: TemporalHistory::Instances(built.document),
        presence,
        cut,
        time_range: built.time_range,
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        error::TemporalError,
        instance_limits::TemporalInstanceLimits,
        read::rendered_entity::EntityHistoryPage,
        result_bound::ResultBound,
        service::TemporalService,
        service_limits::TemporalServiceLimits,
        stub_broker::{StubBroker, limits, service_under, uri},
    };
    use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, instance_identity::InstanceIdentity, temporal_format::TemporalFormat, time_prop::TimeProp};
    use andromeda_ngsi_ld_model::temporal::{StreamExtent, TemporalHistoryRow, TemporalValue, evolution_instance::EvolutionInstance};
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        temporal::{
            compiled::CompiledTemporalQuery,
            instance_page::{InstancePage, PageDirection},
            instance_selection::InstanceSelection,
            representation::Representation,
            window::{TemporalWindow, TimeInterval},
        },
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use chrono::{DateTime, Utc};
    use std::{num::NonZeroU32, slice, sync::Arc};

    fn at(raw: &str) -> DateTime<Utc> {
        raw.parse().expect("a valid instant")
    }

    /// A service paging at 19 instances, answering a `lastN` of up to 19 in
    /// full, and carrying edge groups of up to `edge_group` instances.
    fn service(edge_group: u32) -> TemporalService {
        let limits = limits();
        service_under(
            Arc::new(StubBroker::new([])),
            None,
            TemporalServiceLimits {
                instance_limits: TemporalInstanceLimits {
                    default_page: 19,
                    maximum: 19,
                    edge_group,
                    ..limits.instance_limits
                },
                ..limits
            },
        )
    }

    /// The normalized representation of every instance observed after 2020,
    /// read as `page`.
    fn compiled(page: InstancePage) -> CompiledTemporalQuery {
        CompiledTemporalQuery::builder()
            .entities(EntitySelection::builder().build())
            .window(
                TemporalWindow::builder()
                    .timeproperty(TimeProp::Observed)
                    .interval(TimeInterval::After(at("2020-01-01T00:00:00Z")))
                    .build(),
            )
            .instances(InstanceSelection::builder().page(page).build())
            .representation(
                Representation::builder()
                    .format(TemporalFormat::Normalized)
                    .aggr_methods(Vec::new())
                    .aggr_period_duration(None)
                    .sys_attrs(SysAttrsInclusion::Strip)
                    .build(),
            )
            .build()
    }

    /// `count` instances of `speed` observed at `instant`, valued from `first`
    /// on, in a stream read to `extent`.
    fn simultaneous(count: i64, first: i64, instant: &str, extent: StreamExtent) -> Vec<TemporalHistoryRow> {
        let instant = at(instant);
        (first..first + count)
            .map(|value| TemporalHistoryRow {
                attribute_name: "https://example.com/speed".parse().expect("a valid attribute name"),
                attribute_type: AttributeType::Property,
                dataset_id: None,
                value: TemporalValue::Number {
                    number: JsonNumber::from(value),
                    datatype: None,
                },
                observed_at: Some(instant),
                recorded_at: instant,
                created_at: Some(instant),
                modified_at: Some(instant),
                instance: EvolutionInstance::Attribute(InstanceIdentity::Verbatim(uri(&format!("urn:ngsi-ld:Instance:{value}")))),
                metadata: None,
                stream_extent: extent,
            })
            .collect()
    }

    fn page(entity: &str, rows: Vec<TemporalHistoryRow>) -> EntityHistoryPage {
        EntityHistoryPage {
            entity_id: entity.parse().expect("a valid identifier"),
            rows,
        }
    }

    /// The `speed` values of `page` the response cut at `compiled` keeps.
    fn kept(service: &TemporalService, compiled: &CompiledTemporalQuery, page: &EntityHistoryPage) -> usize {
        let cut = service.common_page_cut(compiled, slice::from_ref(page)).expect("a page within the bound");
        page.rows
            .iter()
            .filter(|row| cut.is_none_or(|settled| settled.cut.keeps(row, TimeProp::Observed)))
            .count()
    }

    /// Clause 6.3.10 resumes a forward page from its `range-end` `DateTime`, which
    /// cannot point inside an instant: 25 instances sharing the instant a page
    /// of 19 ends at are carried whole, and the response is cut there.
    #[test]
    fn a_forward_page_carries_an_edge_group_larger_than_the_maximum_whole() {
        let service = service(100);
        let compiled = compiled(InstancePage::Forward { size: 19 });
        let page = page("urn:ngsi-ld:Vehicle:1", simultaneous(25, 0, "2024-01-01T00:00:00Z", StreamExtent::Cut));
        let cut = service.common_page_cut(&compiled, slice::from_ref(&page)).expect("a group within the bound");
        pretty_assertions::assert_eq!(cut.map(|settled| settled.cut.boundary), Some(at("2024-01-01T00:00:00Z")));
        pretty_assertions::assert_eq!(kept(&service, &compiled, &page), 25);
    }

    /// A `lastN` of 21 above the maximum of 19 pages backwards: the 5 latest
    /// instances and the 25 sharing the instant the page reaches into are
    /// carried whole, the response starting at that instant.
    #[test]
    fn a_backward_page_carries_an_edge_group_larger_than_the_maximum_whole() {
        let service = service(100);
        let compiled = compiled(InstancePage::Backward {
            size: 19,
            requested: NonZeroU32::new(21).expect("a positive lastN"),
        });
        let mut rows = simultaneous(5, 100, "2024-01-02T00:00:00Z", StreamExtent::Cut);
        rows.extend(simultaneous(25, 0, "2024-01-01T00:00:00Z", StreamExtent::Cut));
        let page = page("urn:ngsi-ld:Vehicle:1", rows);
        let cut = service
            .common_page_cut(&compiled, slice::from_ref(&page))
            .expect("a group within the bound")
            .expect("the stream was cut")
            .cut;
        assert!(matches!(cut.direction, PageDirection::Backward { .. }));
        pretty_assertions::assert_eq!(cut.boundary, at("2024-01-01T00:00:00Z"));
        pretty_assertions::assert_eq!(kept(&service, &compiled, &page), 30);
    }

    /// Clause 5.5.6 leaves the threshold of `TooManyResults` to the
    /// implementation: a group of 25 over an edge group bound of 20 is refused,
    /// and the refusal names the setting.
    #[test]
    fn an_edge_group_above_the_bound_is_too_many_results_naming_the_bound() {
        let service = service(20);
        let compiled = compiled(InstancePage::Forward { size: 19 });
        let page = page("urn:ngsi-ld:Vehicle:1", simultaneous(25, 0, "2024-01-01T00:00:00Z", StreamExtent::Cut));
        let refusal = service.common_page_cut(&compiled, slice::from_ref(&page)).expect_err("a group above the bound");
        assert!(
            matches!(
                refusal,
                TemporalError::TooManyResults {
                    matched: 25,
                    maximum: 20,
                    bound: ResultBound::EdgeGroup,
                }
            ),
            "{refusal:?}"
        );
        assert!(refusal.to_string().contains("temporal.max_edge_group"), "{refusal}");
    }

    /// One response is cut at one instant across its entities: vehicle 1 was
    /// cut at its group of 25 on January 1st, so vehicle 2 keeps its 10
    /// instances there whole and leaves out its 30 of January 2nd, which the
    /// bound of 25 therefore does not refuse.
    #[test]
    fn the_common_cut_keeps_each_entity_s_edge_group_whole() {
        let service = service(25);
        let compiled = compiled(InstancePage::Forward { size: 19 });
        let first = page("urn:ngsi-ld:Vehicle:1", simultaneous(25, 0, "2024-01-01T00:00:00Z", StreamExtent::Cut));
        let mut rows = simultaneous(10, 0, "2024-01-01T00:00:00Z", StreamExtent::Cut);
        rows.extend(simultaneous(30, 100, "2024-01-02T00:00:00Z", StreamExtent::Cut));
        let second = page("urn:ngsi-ld:Vehicle:2", rows);
        let pages = [first, second];
        let cut = service
            .common_page_cut(&compiled, &pages)
            .expect("every kept group within the bound")
            .expect("both streams were cut")
            .cut;
        pretty_assertions::assert_eq!(cut.boundary, at("2024-01-01T00:00:00Z"));
        let kept: Vec<usize> = pages
            .iter()
            .map(|page| page.rows.iter().filter(|row| cut.keeps(row, TimeProp::Observed)).count())
            .collect();
        pretty_assertions::assert_eq!(kept, vec![25, 10]);
    }
}
