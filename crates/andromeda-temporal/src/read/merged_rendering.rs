//! Rendering the aggregated representation of every entity a Context Source
//! contributed a part of to a split query, over its merged instances (clause
//! 5.7.4.4).
//!
//! Clause 5.7.4.4 renders such an entity with the "aggregated values of
//! Attribute instances" of the aggregated entity. A source's own aggregate is
//! of its part alone, and two aggregates of the same period are no aggregate
//! of their union, so the sources answered with their instances and the
//! aggregate is computed here, by the store, over the local and answered
//! instances together.

use crate::{
    answered_history::answered_history,
    error::TemporalError,
    grouping_depth::grouping_depth,
    port::read_scope::ScopeReach,
    query_reading::QueryReading,
    query_warning_reason::TemporalQueryWarningReason,
    result_bound::ResultBound,
    service::{TemporalQueryWarning, TemporalService},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_compaction::{aggregated_entity::aggregated_entity, entity_compactor::EntityCompactor};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory};
use andromeda_ngsi_ld_render::temporal::whole_range::WholeRange;
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use chrono::Utc;

/// The aggregated representations of the contributed entities, and the
/// warnings of the parts or entities that could not be rendered.
#[derive(Debug, Default)]
pub struct MergedRendering {
    /// One aggregated representation per entity holding an aggregate.
    pub entities: Vec<TemporalHistory>,
    /// One warning per answered part, or entity, that could not be read or
    /// rendered (clause 6.3.17).
    pub warnings: Vec<TemporalQueryWarning>,
}

impl TemporalService {
    /// Render each of `parts`' entities with the aggregates of its local
    /// instances and the instances the sources answered for it.
    ///
    /// The scope query does not bound the instances here: the Scope events of
    /// a contributed entity may be held by a source, and the filters were
    /// decided over its gathered parts already (Annex C clause C.5.16, clause
    /// 5.7.4.4). Nor does a page: the aggregate is over every part together,
    /// so each entity is aggregated over its whole window, or the `lastN` it
    /// asks for, and the response's common bound is applied to the merge.
    ///
    /// The request's terms and `q` target languages are read as `reading`
    /// says.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the query does not compile, the context
    /// does not resolve, or an entity's aggregation exceeds the grouping bound
    /// (`TooManyResults`); any other failure of one entity or one part is a
    /// warning.
    pub async fn render_merged_aggregations(
        &self,
        target: &ContextTarget,
        params: &TemporalQueryParameters,
        reading: QueryReading<'_>,
        parts: Vec<(Identifier, Vec<TemporalDocument>)>,
    ) -> Result<MergedRendering, TemporalError> {
        let QueryReading { context, language_targeting } = reading;
        let mut compiled = self.compile_temporal_query(params, context, language_targeting).await?;
        compiled.entities.scope_q = None;
        compiled.instances.page = compiled.instances.page.unpaged();
        let compiled = compiled;
        let entity_ids: Vec<Identifier> = parts.iter().map(|(entity_id, _)| entity_id.clone()).collect(); // clone: the catalogue read takes the identifiers by value
        let scope = self.reader.read_scope(target, ScopeReach::Every).await?;
        let local_types = self.reader.entity_types(&scope, &entity_ids).await?;
        let compactor = self.engine.compactor(context);
        let compactor = EntityCompactor::new(&compactor);
        let recorded_at = Utc::now();
        let mut rendering = MergedRendering::default();
        let mut aggregated_entities = Vec::with_capacity(parts.len());
        for (entity_id, answers) in parts {
            let mut rows = Vec::new();
            let mut answered_types = Vec::new();
            for answer in answers {
                match answered_history(answer, recorded_at) {
                    Ok(history) => {
                        rows.extend(history.rows);
                        answered_types.extend(history.entity_types);
                    }
                    Err(error) => rendering.warnings.push(unreadable(&entity_id, error)),
                }
            }
            let entity_types = local_types.get(&entity_id).cloned().unwrap_or(answered_types); // clone: the catalogue's types are read for every entity
            let aggregated = match self
                .reader
                .query_merged_entity_aggregation(&scope, &entity_id, &entity_types, &compiled, &rows)
                .await
            {
                Ok(aggregated) => aggregated,
                Err(error @ TemporalError::TooManyResults { .. }) => return Err(error),
                Err(error) => {
                    rendering.warnings.push(unreadable(&entity_id, error));
                    continue;
                }
            };
            if compiled.representation.format.is_grouped() {
                let groups = aggregated.len() as u64;
                if groups > self.limits.aggregation_grouping_max_groups {
                    return Err(TemporalError::too_many_results(
                        groups,
                        self.limits.aggregation_grouping_max_groups,
                        ResultBound::AggregationGroups,
                    ));
                }
            }
            if !aggregated.is_empty() {
                aggregated_entities.push((entity_id, entity_types, aggregated));
            }
        }
        // Clause 4.5.19.1: an absent period is one whole range for every stream
        // these entities answer with.
        if compiled.representation.aggr_period_duration.is_none() {
            let range = WholeRange::of_response(aggregated_entities.iter().flat_map(|(_, _, rows)| rows.iter()));
            for row in aggregated_entities.iter_mut().flat_map(|(_, _, rows)| rows.iter_mut()) {
                range.close(row);
            }
        }
        for (entity_id, entity_types, aggregated) in aggregated_entities {
            match compactor.names().await {
                Ok(names) => rendering.entities.push(TemporalHistory::Aggregates(aggregated_entity(
                    entity_id,
                    &entity_types,
                    &aggregated,
                    &compiled.representation.aggr_methods,
                    grouping_depth(&compiled),
                    names,
                ))),
                Err(error) => rendering.warnings.push(unreadable(&entity_id, TemporalError::from(error))),
            }
        }
        Ok(rendering)
    }
}

/// The warning of an entity, or a part of it, that could not be rendered.
fn unreadable(entity_id: &Identifier, error: TemporalError) -> TemporalQueryWarning {
    TemporalQueryWarning {
        entity_id: entity_id.clone(), // clone: the warning owns the identifier it names
        reason: TemporalQueryWarningReason::MergedAggregation(error),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
