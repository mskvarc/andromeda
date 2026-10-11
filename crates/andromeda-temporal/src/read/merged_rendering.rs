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
    service::{TemporalQueryWarning, TemporalService},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_compaction::{aggregated_entity::aggregated_entity, entity_compactor::EntityCompactor};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::{aggregated_entity::AggregationLayout, temporal_document::TemporalDocument, temporal_history::TemporalHistory};
use andromeda_ngsi_ld_query::temporal::period_timeline::PeriodTimeline;
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
    /// 5.7.4.4).
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the query does not compile, the context
    /// does not resolve, or an entity's aggregation exceeds the grouping or
    /// the period bound (`TooManyResults`); any other failure of one entity or
    /// one part is a warning.
    pub async fn render_merged_aggregations(
        &self,
        target: &ContextTarget,
        params: &TemporalQueryParameters,
        context: &ContextSpec,
        parts: Vec<(Identifier, Vec<TemporalDocument>)>,
    ) -> Result<MergedRendering, TemporalError> {
        let mut compiled = self.compile_temporal_query(params, context).await?;
        compiled.entities.scope_q = None;
        let compiled = compiled;
        let entity_ids: Vec<Identifier> = parts.iter().map(|(entity_id, _)| entity_id.clone()).collect(); // clone: the catalogue read takes the identifiers by value
        let local_types = self.reader.entity_types(target, &entity_ids).await?;
        let compactor = self.engine.compactor(context);
        let compactor = EntityCompactor::new(&compactor);
        let recorded_at = Utc::now();
        let timeline = PeriodTimeline::of(&compiled, u64::from(self.limits.aggregation_max_periods));
        let mut rendering = MergedRendering::default();
        for (entity_id, answers) in parts {
            let mut rows = Vec::new();
            let mut answered_types = Vec::new();
            for answer in answers {
                match answered_history(answer, recorded_at) {
                    Ok(history) => {
                        rows.extend(history.rows);
                        answered_types.extend(history.entity_types);
                    }
                    Err(error) => rendering.warnings.push(unreadable(&entity_id, &error)),
                }
            }
            let entity_types = local_types.get(&entity_id).cloned().unwrap_or(answered_types); // clone: the catalogue's types are read for every entity
            let aggregated = match self
                .reader
                .query_merged_entity_aggregation(target, &entity_id, &entity_types, &compiled, &rows)
                .await
            {
                Ok(aggregated) => aggregated,
                Err(error @ TemporalError::TooManyResults { .. }) => return Err(error),
                Err(error) => {
                    rendering.warnings.push(unreadable(&entity_id, &error));
                    continue;
                }
            };
            if compiled.representation.format.is_grouped() {
                let groups = aggregated.len() as u64;
                if groups > self.limits.aggregation_grouping_max_groups {
                    return Err(TemporalError::too_many_results(groups, self.limits.aggregation_grouping_max_groups));
                }
            }
            if aggregated.is_empty() {
                continue;
            }
            let names = match compactor.names().await {
                Ok(names) => names,
                Err(error) => {
                    rendering.warnings.push(unreadable(&entity_id, &TemporalError::from(error)));
                    continue;
                }
            };
            let layout = AggregationLayout {
                grouping_depth: grouping_depth(&compiled),
                timeline: timeline.as_ref(),
            };
            let entity = aggregated_entity(entity_id, &entity_types, &aggregated, &compiled.representation.aggr_methods, layout, names)
                .map_err(TemporalError::from_period_limit)?;
            rendering.entities.push(TemporalHistory::Aggregates(entity));
        }
        Ok(rendering)
    }
}

/// The warning of an entity, or a part of it, that could not be rendered.
fn unreadable(entity_id: &Identifier, error: &TemporalError) -> TemporalQueryWarning {
    TemporalQueryWarning {
        entity_id: entity_id.clone(), // clone: the warning owns the identifier it names
        message: format!("the merged aggregation of the entity could not be rendered in full: {error}"),
    }
}

#[cfg(test)]
mod tests;
