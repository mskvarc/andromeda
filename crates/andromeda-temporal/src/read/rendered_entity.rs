//! Rendering the temporal evolution of one entity over a compiled window.
//!
//! Three callers need exactly this and nothing more: the query endpoint renders
//! it once per matching identifier, the clause 4.5.23 join renders it once per
//! Linked Entity it resolves, and both must produce byte-identical
//! representations for the same entity over the same window. Keeping the
//! pipeline — history read, representation build, JSON-LD compaction, array
//! normalisation, `temporalValues` projection — in one place is what
//! guarantees that.
//!
//! The window arrives already compiled rather than as request parameters, so a
//! caller that renders many entities compiles once instead of once per entity.

use crate::{
    error::TemporalError,
    linked::candidates::{TemporalLinkedCandidate, collect_candidates},
    service::TemporalService,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{compaction::NameCompactor, engine::options::CompactOptions};
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, temporal_format::TemporalFormat, uri::Uri};
use andromeda_ngsi_ld_model::temporal::TemporalHistoryRow;
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_ngsi_ld_render::{
    structural_projection::StructuralProjection,
    temporal::{
        array_normalizer::TemporalArrayNormalizer,
        entity::TemporalEntityRenderer,
        history_row::TimeRange,
        page_cut::{PageCut, largest_simultaneous_group},
        values::TemporalValuesRenderer,
    },
};
use indexmap::IndexSet;
use serde_json::Value;
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

/// One entity's temporal representation, with the instants the caller needs
/// in order to build a `Content-Range`.
///
/// The range travels with the value because it is a property of *this*
/// rendering: a caller aggregating many entities takes the widest range across
/// them, which it cannot do once the representations have been flattened into
/// a collection.
#[derive(Debug)]
pub(crate) struct RenderedTemporalEntity {
    /// The compacted temporal representation.
    pub(crate) value: Value,
    /// Whether the window selected any history at all.
    pub(crate) presence: HistoryPresence,
    /// The cut the representation was made at, present when it is part of a
    /// partial response (clause 6.3.10).
    pub(crate) cut: Option<PageCut>,
    /// The earliest and latest instants the representation actually carries.
    pub(crate) time_range: Option<TimeRange>,
    /// The Linked Entities this entity names, read off the representation
    /// before any format projection narrowed it.
    ///
    /// Clause 4.5.8 collapses each attribute's instances into parallel value
    /// and timestamp arrays, which erases the `object` members a join needs to
    /// follow. Collecting here — while the normalized form still exists — is
    /// what lets `join=flat` work in every representation rather than silently
    /// finding nothing outside the normalized one.
    pub(crate) candidates: Vec<TemporalLinkedCandidate>,
}

/// One entity's history as its page read it, before the response's common cut
/// is applied.
#[derive(Debug)]
pub(crate) struct EntityHistoryPage {
    /// The entity the history belongs to.
    pub(crate) entity_id: Uri,
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
    /// Render one entity's temporal evolution over an already-compiled window.
    ///
    /// The aggregated representation of clause 4.5.19 bypasses JSON-LD
    /// compaction entirely, because the aggregation keys are not IRIs and
    /// compaction would drop them; that path is taken here so both callers get
    /// the same treatment rather than one of them silently supporting fewer
    /// formats. Any other representation is the entity's page, cut on its own
    /// (clause 6.3.10): a caller rendering several entities into one response
    /// reads their pages and cuts them together instead.
    ///
    /// A window that selects nothing still renders — into a temporal entity
    /// carrying no attribute instances — and says so through
    /// [`RenderedTemporalEntity::presence`], because whether that is a result,
    /// an omission or a 404 is the caller's decision.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the history cannot be read, a page would
    /// carry more instances sharing one timestamp than the broker's maximum,
    /// the representation cannot be compacted, or the `temporalValues`
    /// projection fails. Each caller decides whether that ends the operation or
    /// merely drops one entity from it.
    pub(crate) async fn render_one_temporal_entity(
        &self,
        target: &ContextTarget,
        entity_id: &Uri,
        entity_types: &[EntityTypeName],
        compiled: &CompiledTemporalQuery,
        names: &NameCompactor<'_>,
    ) -> Result<RenderedTemporalEntity, TemporalError> {
        if compiled.representation.format.is_aggregated() {
            return self.render_aggregated_entity(target, entity_id, entity_types, compiled, names).await;
        }
        let page = self.read_history_page(target, entity_id, compiled).await?;
        let cut = self.common_page_cut(compiled, slice::from_ref(&page))?;
        self.render_history_page(page, entity_types, compiled, names, cut).await
    }

    /// Read the page of one entity's history the compiled query selects.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Persistence`] when the history cannot be read.
    pub(crate) async fn read_history_page(
        &self,
        target: &ContextTarget,
        entity_id: &Uri,
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
        let maximum = self.instance_limits.maximum;
        for page in pages {
            let kept = page.rows.iter().filter(|row| cut.is_none_or(|cut| cut.keeps(row, timeproperty)));
            let group = largest_simultaneous_group(kept, timeproperty);
            if group > usize::try_from(maximum).unwrap_or(usize::MAX) {
                return Err(TemporalError::too_many_results(u64::try_from(group).unwrap_or(u64::MAX), u64::from(maximum)));
            }
        }
        Ok(cut)
    }

    /// Render one entity's page at the response's common cut.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the representation cannot be compacted or
    /// the `temporalValues` projection fails.
    pub(crate) async fn render_history_page(
        &self,
        page: EntityHistoryPage,
        entity_types: &[EntityTypeName],
        compiled: &CompiledTemporalQuery,
        names: &NameCompactor<'_>,
        cut: Option<PageCut>,
    ) -> Result<RenderedTemporalEntity, TemporalError> {
        let presence = page.presence();
        let built = TemporalEntityRenderer::new(&page.entity_id, entity_types, &page.rows, compiled, cut).render();
        // Clause 5.7.3 / 5.5.7: the representation is compacted against the
        // request @context, which is the core context alone when the client
        // supplies none. A term the resolved context does not declare stays a
        // full IRI; it is not truncated to a bare fragment, since a fragment is
        // not a term the client could expand back (clause 5.7.6 renders type
        // and attribute discovery the same way).
        let compacted = names.compactor().compact(built.entity, CompactOptions::default()).await?;
        let mut compacted = TemporalArrayNormalizer::new(compacted).render();
        // Clause 4.21, `id` / `type` half: the attribute half of the
        // projection already ran inside `TemporalEntityRenderer` on the
        // expanded representation, keyed by attribute IRI; `id` and `type`
        // only exist as such once compaction has turned `@id` / `@type` into
        // them, so their half of the structural rule runs here instead.
        StructuralProjection::from(&compiled.projection).prune(&mut compacted);
        // Read the Linked Entities off the normalized form, before the clause
        // 4.5.8 projection below erases the `object` members they live in.
        let candidates = collect_candidates(slice::from_ref(&compacted), &IndexSet::new());

        let value = if compiled.representation.format == TemporalFormat::TemporalValues {
            TemporalValuesRenderer::new(compacted, compiled.window.timeproperty)
                .render()
                .map_err(|error| TemporalError::rendering(error.to_string()))?
        } else {
            compacted
        };

        Ok(RenderedTemporalEntity {
            value,
            presence,
            cut,
            time_range: built.time_range,
            candidates,
        })
    }

    /// Render the clause 4.5.19 aggregated representation of one entity.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the aggregation cannot be read, the
    /// grouped extension's group bound is exceeded, or a name cannot be
    /// compacted.
    async fn render_aggregated_entity(
        &self,
        target: &ContextTarget,
        entity_id: &Uri,
        entity_types: &[EntityTypeName],
        compiled: &CompiledTemporalQuery,
        names: &NameCompactor<'_>,
    ) -> Result<RenderedTemporalEntity, TemporalError> {
        let rows = self.reader.query_entity_aggregation(target, entity_id, compiled).await?;
        // The grouped extension refuses a runaway grouping off the count of
        // distinct groups it produced — one per row — mirroring the
        // delete-by-query bound (study §4.9). `TooManyResults` is fatal to
        // the whole operation, so the query loop must not demote it to a
        // per-entity warning.
        if compiled.representation.format.is_grouped() {
            let groups = rows.len() as u64;
            if groups > self.aggregation_grouping_max_groups {
                return Err(TemporalError::too_many_results(groups, self.aggregation_grouping_max_groups));
            }
        }
        let presence = presence_of(rows.is_empty());
        let value = self.build_aggregated_compacted(entity_id, entity_types, &rows, compiled, names).await?;
        Ok(RenderedTemporalEntity {
            value,
            presence,
            cut: None,
            time_range: None,
            // Clause 4.5.19 aggregates numeric observations into buckets.
            // A Relationship has nothing to aggregate and so never appears
            // in an aggregated representation, which leaves a join over one
            // with nothing to follow.
            candidates: Vec::new(),
        })
    }
}
