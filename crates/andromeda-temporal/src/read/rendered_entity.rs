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

use crate::{error::TemporalError, grouping_depth::grouping_depth, service::TemporalService, temporal_invariant::TemporalInvariant};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_compaction::{aggregated_entity::aggregated_entity, entity_compactor::EntityCompactor};
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
const fn presence_of(is_empty: bool) -> HistoryPresence {
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
    pub(crate) cut: Option<PageCut>,
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
    /// silently supporting fewer formats. Any other representation is the
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
    /// carry more instances sharing one timestamp than the broker's maximum,
    /// or an aggregate's names cannot be compacted. Each caller decides
    /// whether that ends the operation or merely drops one entity from it.
    pub(crate) async fn render_one_temporal_entity(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        entity_types: &[EntityTypeName],
        compiled: &CompiledTemporalQuery,
        compactor: &EntityCompactor<'_>,
    ) -> Result<RenderedTemporalEntity, TemporalError> {
        if compiled.representation.format.is_aggregated() {
            return self.render_aggregated_entity(target, entity_id, entity_types, compiled, compactor).await;
        }
        let page = self.read_history_page(target, entity_id, compiled).await?;
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
        target: &ContextTarget,
        entity_id: &Identifier,
        compiled: &CompiledTemporalQuery,
    ) -> Result<EntityHistoryPage, TemporalError> {
        let rows = self.reader.query_entity_history(target, entity_id, compiled).await?.into_rows();
        Ok(EntityHistoryPage {
            entity_id: entity_id.clone(), // clone: the page owns the identifier it is rendered under.
            rows,
        })
    }

    /// The one cut every entity of a response is made at, or `None` when the
    /// response is complete (clause 6.3.10).
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::TooManyResults`] when a page would carry more
    /// instances sharing one timestamp than the broker's maximum: a page never
    /// splits such a group, so it is the one thing that could grow a page
    /// without bound.
    pub(crate) fn common_page_cut(&self, compiled: &CompiledTemporalQuery, pages: &[EntityHistoryPage]) -> Result<Option<PageCut>, TemporalError> {
        let Some(direction) = compiled.instances.page.direction() else {
            return Ok(None);
        };
        let timeproperty = compiled.window.timeproperty;
        let cut = PageCut::common(pages.iter().filter_map(|page| PageCut::of_page(&page.rows, timeproperty, direction)));
        let maximum = self.limits.instance_limits.maximum;
        for page in pages {
            let kept = page.rows.iter().filter(|row| cut.is_none_or(|cut| cut.keeps(row, timeproperty)));
            let group = largest_simultaneous_group(kept, timeproperty);
            if group > usize::try_from(maximum).unwrap_or(usize::MAX) {
                return Err(TemporalError::too_many_results(u64::try_from(group).unwrap_or(u64::MAX), u64::from(maximum)));
            }
        }
        Ok(cut)
    }

    /// Read the clause 4.5.19 aggregated representation of one entity.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the aggregation cannot be read, the
    /// grouped extension's group bound is exceeded, or a name cannot be
    /// compacted.
    async fn render_aggregated_entity(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        entity_types: &[EntityTypeName],
        compiled: &CompiledTemporalQuery,
        compactor: &EntityCompactor<'_>,
    ) -> Result<RenderedTemporalEntity, TemporalError> {
        let rows = self.reader.query_entity_aggregation(target, entity_id, compiled).await?;
        // The grouped extension refuses a runaway grouping off the count of
        // distinct groups it produced -- one per row -- mirroring the
        // delete-by-query bound. `TooManyResults` is fatal
        // to the whole operation, so the query loop must not demote it to a
        // per-entity warning.
        if compiled.representation.format.is_grouped() {
            let groups = rows.len() as u64;
            if groups > self.limits.aggregation_grouping_max_groups {
                return Err(TemporalError::too_many_results(groups, self.limits.aggregation_grouping_max_groups));
            }
        }
        let presence = presence_of(rows.is_empty());
        let aggregates = aggregated_entity(
            entity_id.clone(), // clone: the aggregated entity owns the identifier it names
            entity_types,
            &rows,
            &compiled.representation.aggr_methods,
            grouping_depth(compiled),
            compactor.names().await?,
        );
        Ok(RenderedTemporalEntity {
            history: TemporalHistory::Aggregates(aggregates),
            presence,
            cut: None,
            time_range: None,
        })
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
    cut: Option<PageCut>,
) -> Result<RenderedTemporalEntity, TemporalError> {
    let presence = page.presence();
    let EntityHistoryPage { entity_id, rows } = page;
    let built = TemporalEntityRenderer::new(&entity_id, entity_types, rows, compiled, cut)
        .render()
        .map_err(|error| TemporalError::from(TemporalInvariant::RenderedInstance(error)))?;
    Ok(RenderedTemporalEntity {
        history: TemporalHistory::Instances(built.document),
        presence,
        cut,
        time_range: built.time_range,
    })
}
