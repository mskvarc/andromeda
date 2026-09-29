//! Query Temporal Evolution of Entities (clause 5.7.4): the matching entities of
//! one page, each rendered at the page's common clause 6.3.10 cut.

use crate::{
    error::TemporalError,
    linked::candidates::TemporalLinkedCandidate,
    read::{
        dataset_step::DatasetStep,
        ensure_aggregation_supported,
        rendered_entity::{HistoryPresence, RenderedTemporalEntity},
    },
    service::{TemporalQueryResult, TemporalQueryWarning, TemporalRange, TemporalService},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{compaction::NameCompactor, context_spec::ContextSpec};
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use andromeda_telemetry::temporal::{self, TemporalOperation};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

impl TemporalService {
    /// Query temporal entities matching the given filter and temporal parameters.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Request`] when the parameters do not satisfy
    /// clause 5.7.4 or name an attribute that is ineligible for a requested
    /// aggregation method (clause 5.7.4.4),
    /// [`TemporalError::OperationNotSupported`] when the selected reader
    /// refuses one of those methods, [`TemporalError::Internal`] when the
    /// reader reports a stage-one outcome that paginated ahead of a residual
    /// predicate, and [`TemporalError::Persistence`] when the history cannot
    /// be read. A single entity that fails to render is reported as a warning
    /// on the result rather than failing the query.
    pub async fn query(&self, target: &ContextTarget, params: &TemporalQueryParameters, context: &ContextSpec) -> Result<TemporalQueryResult, TemporalError> {
        let timer = temporal::start(TemporalOperation::Query);
        let result = async {
            params.validate(true)?;

            let compiled = self.compile_temporal_query(params, context).await?;

            ensure_aggregation_supported(self.reader.as_ref(), &compiled.representation.aggr_methods)?;
            self.ensure_requested_attributes_aggregable(target, &compiled).await?;
            let id_outcome = self.reader.query_entity_ids(target, &compiled).await?;
            let (entity_ids, total_count) = id_outcome
                .complete(compiled.pagination)
                .map_err(|error| TemporalError::internal(error.to_string()))?;

            // One batch read of the temporal catalogue, which owns the entity types
            // (clauses 4.5.1 and 4.5.6) and answers for entities whose current
            // state a clause 5.6.6 deletion has already removed.
            let entity_types_map: HashMap<Identifier, Vec<EntityTypeName>> = self.reader.entity_types(target, &entity_ids).await?;

            let dataset_step = DatasetStep::of(&compiled);
            let names = NameCompactor::new(&self.engine, context);
            let mut warnings = Vec::new();
            let rendered = if compiled.representation.format.is_aggregated() {
                let mut rendered = Vec::with_capacity(entity_ids.len());
                for entity_id in entity_ids {
                    let entity_types = entity_types_map.get(&entity_id).map(Vec::as_slice).unwrap_or_default();
                    let outcome = self.render_one_temporal_entity(target, &entity_id, entity_types, &compiled, &names).await;
                    rendered.extend(settle_entity(entity_id, outcome, &mut warnings)?);
                }
                rendered
            } else {
                // Clause 6.3.10: one response carries one `Content-Range`, so every
                // entity of the page is cut at the same instant. Every page is read
                // before any is rendered, because the cut depends on all of them.
                let mut pages = Vec::with_capacity(entity_ids.len());
                for entity_id in entity_ids {
                    match self.read_history_page(target, &entity_id, &compiled).await {
                        Ok(page) => pages.push(page),
                        Err(error) => {
                            settle_entity(entity_id, Err(error), &mut warnings)?;
                        }
                    }
                }
                let cut = self.common_page_cut(&compiled, &pages)?;
                let mut rendered = Vec::with_capacity(pages.len());
                for page in pages {
                    let entity_types = entity_types_map.get(&page.entity_id).map(Vec::as_slice).unwrap_or_default();
                    // clone: a render failure names the entity the page was consumed for.
                    let entity_id = page.entity_id.clone();
                    let outcome = self.render_history_page(page, entity_types, &compiled, &names, cut).await;
                    rendered.extend(settle_entity(entity_id, outcome, &mut warnings)?);
                }
                rendered
            };

            let mut entities = Vec::with_capacity(rendered.len());
            let mut linked_candidates: Vec<TemporalLinkedCandidate> = Vec::new();
            let mut cut = None;
            let mut overall_min: Option<DateTime<Utc>> = None;
            let mut overall_max: Option<DateTime<Utc>> = None;
            for rendered in rendered {
                // An entity the window selects no history for is simply not part
                // of the result set. One the page's common cut emptied is: its
                // history continues on the next page. So is one only the
                // `datasetId` parameter emptied: clause 5.7.4.4 applies it at
                // step S8, after stage one selected the entity, and S8 removes
                // "all Attributes without remaining Attribute instances", never
                // the Entity.
                //
                // Clause 5.7.4.5 reduces "every Entity within the payload body"
                // to the members the projection defines. An Entity it would
                // reduce to none of them was never selected
                // (`EntitySelection::projected_members`), so none is left out
                // here, after the page was cut.
                if rendered.presence == HistoryPresence::Absent && dataset_step == DatasetStep::Absent {
                    continue;
                }
                cut = cut.or(rendered.cut);
                if let Some((min_t, max_t)) = rendered.time_range {
                    overall_min = Some(overall_min.map_or(min_t, |seen: DateTime<Utc>| seen.min(min_t)));
                    overall_max = Some(overall_max.map_or(max_t, |seen: DateTime<Utc>| seen.max(max_t)));
                }
                linked_candidates.extend(rendered.candidates);
                entities.push(rendered.value);
            }

            let any_truncated = cut.is_some();
            let range = cut.map(|cut| TemporalRange {
                interval: compiled.window.interval,
                observed: overall_min.zip(overall_max),
                direction: cut.direction,
            });

            Ok(TemporalQueryResult {
                entities,
                total_count,
                any_truncated,
                range,
                warnings,
                linked_candidates,
            })
        }
        .await;
        timer.finish_result(&result);
        result
    }
}

/// Settle one entity of a temporal query: its representation, nothing, or a
/// warning.
///
/// One entity that cannot be read or rendered degrades the result set rather
/// than the query -- the rest of the matches still answer -- except for a
/// `TooManyResults` (403), which refuses the whole operation rather than
/// dropping one entity from it.
///
/// # Errors
///
/// Returns the [`TemporalError::TooManyResults`] the entity raised.
fn settle_entity(
    entity_id: Identifier,
    outcome: Result<RenderedTemporalEntity, TemporalError>,
    warnings: &mut Vec<TemporalQueryWarning>,
) -> Result<Option<RenderedTemporalEntity>, TemporalError> {
    match outcome {
        Ok(rendered) => Ok(Some(rendered)),
        Err(error @ TemporalError::TooManyResults { .. }) => Err(error),
        Err(error) => {
            tracing::warn!(error = %error, entity_id = %entity_id, "Failed to render the temporal evolution of an entity");
            warnings.push(TemporalQueryWarning {
                entity_id, // moved: the warning owns the identifier this entity finished with
                message: error.to_string(),
            });
            Ok(None)
        }
    }
}
