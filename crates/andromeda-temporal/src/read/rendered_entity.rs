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
use andromeda_jsonld::{context_spec::ContextSpec, engine::options::CompactOptions};
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, temporal_format::TemporalFormat, uri::Uri};
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_ngsi_ld_render::{
    structural_projection::StructuralProjection,
    temporal::{array_normalizer::TemporalArrayNormalizer, entity::TemporalEntityRenderer, history_row::TimeRange, values::TemporalValuesRenderer},
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
/// A query drops it from the result set; a retrieval returns it, unless an
/// `attrs` filter was what emptied it, in which case clause 5.7.2 makes it a
/// 404. Reporting the fact rather than deciding it keeps that policy with the
/// endpoints that own it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryPresence {
    /// The window selected at least one observation.
    Present,
    /// The window selected nothing.
    Absent,
}

/// One entity's temporal representation, with the truncation facts the caller
/// needs in order to build a `Content-Range`.
///
/// The truncation members travel with the value because they are properties of
/// *this* rendering: a caller aggregating many entities takes the widest range
/// and the strongest truncation across them, which it cannot do once the
/// representations have been flattened into a collection.
#[derive(Debug)]
pub(crate) struct RenderedTemporalEntity {
    /// The compacted temporal representation.
    pub(crate) value: Value,
    /// Whether the window selected any history at all.
    pub(crate) presence: HistoryPresence,
    /// Whether the broker's `lastN` safety cap cut the history short.
    pub(crate) truncated: bool,
    /// The earliest and latest instants the representation actually carries.
    pub(crate) time_range: Option<TimeRange>,
    /// The largest partition count the reader reported, when it reported one.
    pub(crate) max_partition_total: Option<i64>,
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

impl TemporalService {
    /// Render one entity's temporal evolution over an already-compiled window.
    ///
    /// The aggregated representation of clause 4.5.19 bypasses JSON-LD
    /// compaction entirely, because the aggregation keys are not IRIs and
    /// compaction would drop them; that path is taken here so both callers get
    /// the same treatment rather than one of them silently supporting fewer
    /// formats.
    ///
    /// A window that selects nothing still renders — into a temporal entity
    /// carrying no attribute instances — and says so through
    /// [`RenderedTemporalEntity::presence`], because whether that is a result,
    /// an omission or a 404 is the caller's decision.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the history cannot be read, the
    /// representation cannot be compacted, or the `temporalValues` projection
    /// fails. Each caller decides whether that ends the operation or merely
    /// drops one entity from it.
    pub(crate) async fn render_one_temporal_entity(
        &self,
        target: &ContextTarget,
        entity_id: &Uri,
        entity_types: &[EntityTypeName],
        compiled: &CompiledTemporalQuery,
        context: &ContextSpec,
    ) -> Result<RenderedTemporalEntity, TemporalError> {
        if compiled.representation.format == TemporalFormat::AggregatedValues {
            let rows = self.reader.query_entity_aggregation(target, entity_id, compiled).await?;
            let presence = presence_of(rows.is_empty());
            let value = self.build_aggregated_compacted(entity_id, entity_types, &rows, compiled, context).await?;
            return Ok(RenderedTemporalEntity {
                value,
                presence,
                truncated: false,
                time_range: None,
                max_partition_total: None,
                // Clause 4.5.19 aggregates numeric observations into buckets.
                // A Relationship has nothing to aggregate and so never appears
                // in an aggregated representation, which leaves a join over one
                // with nothing to follow.
                candidates: Vec::new(),
            });
        }

        let rows = self.reader.query_entity_history(target, entity_id, compiled).await?.into_rows();
        let presence = presence_of(rows.is_empty());

        let built = TemporalEntityRenderer::new(entity_id, entity_types, &rows, compiled).render();
        // Clause 5.7.3 / 5.5.7: the representation is compacted against the
        // request @context, which is the core context alone when the client
        // supplies none. A term the resolved context does not declare stays a
        // full IRI; it is not truncated to a bare fragment, since a fragment is
        // not a term the client could expand back (clause 5.7.6 renders type
        // and attribute discovery the same way).
        let compacted = self.engine.compact(built.entity, context, CompactOptions::default()).await?;
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
            TemporalValuesRenderer::new(compacted)
                .render()
                .map_err(|error| TemporalError::rendering(error.to_string()))?
        } else {
            compacted
        };

        Ok(RenderedTemporalEntity {
            value,
            presence,
            truncated: built.truncated,
            time_range: built.time_range,
            max_partition_total: built.max_partition_total,
            candidates,
        })
    }
}
