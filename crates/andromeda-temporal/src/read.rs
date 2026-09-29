//! Temporal retrieval and query execution policy.

use crate::{
    aggregation_eligibility::ensure_aggregation_eligible,
    error::TemporalError,
    port::history_reader::TemporalHistoryReader,
    read::{dataset_step::DatasetStep, rendered_entity::HistoryPresence},
    requested_identifiers::RequestedIdentifiers,
    service::{TemporalRange, TemporalReadResult, TemporalService},
    temporal_not_found_reason::TemporalNotFoundReason,
    temporal_operation_unsupported::TemporalOperationUnsupported,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{
    compaction::NameCompactor,
    context_spec::ContextSpec,
    engine::{expanded_input::ExpandedInput, options::CompactOptions},
};
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::temporal::TemporalAggregationRow;
use andromeda_ngsi_ld_query::{
    pagination::{CountPolicy, Pagination},
    temporal::compiled::CompiledTemporalQuery,
};
use andromeda_ngsi_ld_render::{
    structural_projection::{RenderedEntityMembers, rendered_entity_members},
    temporal::aggregation::TemporalAggregationRenderer,
};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use andromeda_ngsi_ld_vocabulary::expanded::classes as expanded_classes;
use andromeda_telemetry::temporal::{self, TemporalOperation};
use serde_json::{Value, json};
use std::{collections::HashMap, slice::from_ref};

impl TemporalService {
    /// Retrieve and render one temporal entity under the supplied query parameters.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Request`] when the parameters do not satisfy
    /// clause 5.7.3 or name an attribute that is ineligible for a requested
    /// aggregation method (clause 5.7.4.4),
    /// [`TemporalError::OperationNotSupported`] when the selected reader
    /// refuses one of those methods, [`TemporalError::NotFound`] when the
    /// entity has no Temporal Evolution or an `attrs` filter selects none of
    /// it (clause 5.7.2), and [`TemporalError::Persistence`] or
    /// [`TemporalError::Rendering`] when the history cannot be read or
    /// compacted.
    pub async fn get(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        params: &TemporalQueryParameters,
        context: &ContextSpec,
    ) -> Result<TemporalReadResult, TemporalError> {
        let timer = temporal::start(TemporalOperation::Retrieve);
        let result = async {
            params.validate(false)?;

            let compiled = self.compile_temporal_query(params, context).await?;
            ensure_aggregation_supported(self.reader.as_ref(), &compiled.representation.aggr_methods)?;
            self.ensure_requested_attributes_aggregable(target, &compiled).await?;

            if !self.reader.entity_exists(target, entity_id).await? {
                return Err(TemporalError::NotFound(TemporalNotFoundReason::Entity { id: entity_id.clone() }));
            }

            // Clauses 4.5.1 and 4.5.6: the temporal catalogue is the authority for
            // an entity's types, so the mandatory `type` member survives the clause
            // 5.6.6 deletion that removes current state and leaves the Temporal
            // Evolution standing.
            let entity_types = self
                .reader
                .entity_types(target, from_ref(entity_id))
                .await?
                .remove(entity_id)
                .unwrap_or_default();

            let rendered = self
                .render_one_temporal_entity(target, entity_id, &entity_types, &compiled, &NameCompactor::new(&self.engine, context))
                .await?;

            // Clause 5.7.3.4, step S1: "if the time series, for all the
            // concerned Attributes of an Entity, does not include data
            // corresponding to the temporal query interval, then such Entity
            // shall be removed from S, thus it shall not appear in the final
            // result set" -- the concerned Attributes being those `attrs` names,
            // "or all if none are specified". A restriction that leaves nothing
            // therefore leaves no Temporal Evolution to represent (clause
            // 5.7.3.5), which is ResourceNotFound whether or not `attrs` was
            // given. With neither a window nor `attrs` nothing is removed, so an
            // entity whose evolution holds no attribute still answers itself.
            let restricted = compiled.window.interval.is_bounded() || !compiled.instances.attr_filter.is_empty();
            if restricted && self.window_presence(target, entity_id, &compiled, rendered.presence).await? == HistoryPresence::Absent {
                return Err(TemporalError::NotFound(TemporalNotFoundReason::NoInstanceSelected {
                    entity: entity_id.clone(),
                }));
            }

            // Clause 5.7.3.5 reduces the Temporal Evolution "down to only contain
            // the defined Entity members", and Tables 6.4.3.2-1 and 5.2.14.1-1 make
            // `attrs` "a synonym for pick": a projection that leaves no member at
            // all is the same `ResourceNotFound` the `attrs` rule above raises,
            // which clause 5.7.1.4 states in full for the non-temporal twin.
            let projected = !compiled.projection.pick.is_empty() || !compiled.projection.omit.is_empty();
            if projected && rendered_entity_members(&rendered.value) == RenderedEntityMembers::None {
                return Err(TemporalError::NotFound(TemporalNotFoundReason::ProjectionLeftNothing {
                    entity: entity_id.clone(),
                }));
            }

            // Clause 6.3.10: a page that left instances out is a 206 carrying the
            // `Content-Range` its client resumes from.
            let range = rendered.cut.map(|cut| TemporalRange {
                interval: compiled.window.interval,
                observed: rendered.time_range,
                direction: cut.direction,
            });

            Ok(TemporalReadResult {
                value: rendered.value,
                range,
                linked_candidates: rendered.candidates,
            })
        }
        .await;
        timer.finish_result(&result);
        result
    }

    /// Whether clause 5.7.3.4 step S1 -- the temporal window and the `attrs`
    /// selection -- leaves any of the entity's history, given the `presence`
    /// the rendered answer reports.
    ///
    /// The `datasetId` step runs after S1 and removes "the complete Attribute
    /// from S1", never the Entity, so an answer the dataset filter emptied is
    /// judged again without it: the entity then stands, reduced to no
    /// Attribute, rather than being not found.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Persistence`] when the history cannot be read.
    async fn window_presence(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        compiled: &CompiledTemporalQuery,
        presence: HistoryPresence,
    ) -> Result<HistoryPresence, TemporalError> {
        if presence == HistoryPresence::Present || DatasetStep::of(compiled) == DatasetStep::Absent {
            return Ok(presence);
        }
        // clone: the S1 probe reads the same query without the dataset step that follows it
        let mut window_only = compiled.clone();
        window_only.instances.dataset_id_filter.clear();
        let window_only = window_only;
        Ok(self.read_history_page(target, entity_id, &window_only).await?.presence())
    }

    /// Build an aggregated temporal response in compacted form, bypassing JSON-LD compaction.
    /// Aggregation keys (avg, min, etc.) aren't valid JSON-LD IRIs and would be dropped by `compact()`.
    async fn build_aggregated_compacted(
        &self,
        entity_id: &Identifier,
        entity_types: &[EntityTypeName],
        rows: &[TemporalAggregationRow],
        compiled: &CompiledTemporalQuery,
        names: &NameCompactor<'_>,
    ) -> Result<Value, TemporalError> {
        // Build aggregation data keyed by attribute IRI
        let aggr_data = TemporalAggregationRenderer::new(rows, compiled).render();

        // Compact each attribute IRI individually by building a 1-key skeleton and compacting it
        let mut iri_to_short: HashMap<AttributeName, AttributeName> = HashMap::new();
        for attr_iri in aggr_data.keys() {
            let mut skeleton = serde_json::Map::new();
            skeleton.insert("@id".to_string(), json!(entity_id));
            skeleton.insert(attr_iri.to_string(), json!([{"@type": [expanded_classes::PROPERTY]}]));
            if let Ok(compacted) = names
                .compactor()
                .compact(ExpandedInput::Assembled(Value::Object(skeleton)), CompactOptions::default())
                .await
                && let Some(obj) = compacted.as_object()
            {
                for key in obj.keys() {
                    if !["id", "type", "@context"].contains(&key.as_str()) {
                        if let Ok(short) = key.parse() {
                            iri_to_short.insert(attr_iri.clone(), short); // clone: the index owns its attribute key.
                        }
                        break;
                    }
                }
            }
            if !iri_to_short.contains_key(attr_iri) {
                // `compact_attr_iri` returns the term the resolved context
                // declares, or the full IRI verbatim when none does (clause
                // 5.7.3): an undeclared attribute is not truncated to a bare
                // fragment the client could not expand back.
                let short = names.compact_attr_iri(attr_iri.as_str()).await;
                if short != attr_iri.as_str()
                    && let Ok(short) = short.parse()
                {
                    iri_to_short.insert(attr_iri.clone(), short); // clone: the index owns its attribute key.
                }
            }
        }

        // Compact entity type by compacting a minimal type skeleton
        let type_skeleton = json!({"@id": entity_id, "@type": entity_types});
        // The type is compacted against the request @context; an undeclared
        // type stays a full IRI rather than a bare fragment (clause 5.7.3).
        let compacted_type = if let Ok(compacted) = names
            .compactor()
            .compact(ExpandedInput::Assembled(type_skeleton), CompactOptions::default())
            .await
        {
            compacted.as_object().and_then(|o| o.get("type")).cloned().unwrap_or(json!(entity_types)) // clone: need owned Value from borrowed compacted object
        } else {
            json!(entity_types)
        };

        // Build the final result
        let mut result = serde_json::Map::new();
        result.insert("id".to_string(), json!(entity_id));
        result.insert("type".to_string(), compacted_type);

        for (attr_iri, attr_obj) in aggr_data {
            let short_name = iri_to_short.get(&attr_iri).unwrap_or(&attr_iri);
            result.insert(short_name.to_string(), attr_obj);
        }

        Ok(Value::Object(result))
    }

    /// Execute stage 1 entity ID resolution without fetching temporal history.
    ///
    /// Used to build `EntityMaps` that represent the full result set (spec 5.5.14).
    /// Runs only the lightweight stage 1 SQL (entity ID resolution) without fetching
    /// temporal history, ensuring the `EntityMap` captures ALL matching IDs, not just the
    /// current page.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Request`] when the parameters do not satisfy
    /// clause 5.7.4, [`TemporalError::OperationNotSupported`] when the
    /// selected reader refuses a requested aggregation method,
    /// [`TemporalError::Internal`] when the reader reports a stage-one outcome
    /// that paginated ahead of a residual predicate, and
    /// [`TemporalError::Persistence`] when the identifiers cannot be read.
    pub async fn query_ids(
        &self,
        target: &ContextTarget,
        params: &TemporalQueryParameters,
        context: &ContextSpec,
        identifiers: RequestedIdentifiers,
    ) -> Result<Vec<Identifier>, TemporalError> {
        let timer = temporal::start(TemporalOperation::QueryIds);
        let result = async {
            params.validate(true)?;
            let compiled = self.compile_temporal_query(params, context).await?;
            let mut unpaged = compiled.clone();
            unpaged.pagination = Pagination {
                limit: u32::MAX,
                offset: 0,
                count: CountPolicy::Omit,
            };
            match identifiers {
                RequestedIdentifiers::Narrow => {}
                RequestedIdentifiers::Ignore => unpaged.entities.without_identifier_narrowing(),
            }
            ensure_aggregation_supported(self.reader.as_ref(), &unpaged.representation.aggr_methods)?;
            let outcome = self.reader.query_entity_ids(target, &unpaged).await?;
            let (identifiers, _total) = outcome
                .complete(unpaged.pagination)
                .map_err(|error| TemporalError::internal(error.to_string()))?;
            Ok(identifiers)
        }
        .await;
        timer.finish_result(&result);
        result
    }
}

impl TemporalService {
    /// Clause 5.7.4.4: an aggregated representation over an attribute that is
    /// not eligible for one of the requested methods is an `InvalidRequest`,
    /// decided from the families and value shapes the attribute has been
    /// recorded with before any history is read. Only the attributes the
    /// request named can be checked ahead of the read; a request over every
    /// attribute is not, which is why the statement itself must also refuse to
    /// fold a value-derived aggregate for a family Table 4.5.19.1-3 marks N/A.
    async fn ensure_requested_attributes_aggregable(&self, target: &ContextTarget, compiled: &CompiledTemporalQuery) -> Result<(), TemporalError> {
        if !compiled.representation.format.is_aggregated() || compiled.instances.attr_filter.is_empty() {
            return Ok(());
        }
        let attributes = compiled
            .instances
            .attr_filter
            .iter()
            .map(|iri| iri.as_str().parse::<AttributeName>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| TemporalError::internal(format!("an expanded attribute IRI is not an attribute name: {error}")))?;
        let recorded = self.reader.recorded_attribute_shapes(target, &attributes).await?;
        ensure_aggregation_eligible(&attributes, &compiled.representation.aggr_methods, &recorded).map_err(TemporalError::from)
    }
}

/// Reject unsupported aggregation before invoking any reader query operation.
fn ensure_aggregation_supported(reader: &dyn TemporalHistoryReader, requested: &[AggrMethod]) -> Result<(), TemporalError> {
    let refused = reader.refused_aggregation_methods(requested);
    if refused.is_empty() {
        Ok(())
    } else {
        Err(TemporalError::OperationNotSupported(TemporalOperationUnsupported::ReaderRefusedAggregation {
            methods: refused,
        }))
    }
}

/// Whether a temporal read carries the `datasetId` step.
mod dataset_step;
/// Query Temporal Evolution of Entities (clause 5.7.4).
mod query;
/// Rendering the temporal evolution of one entity over a compiled window.
pub(crate) mod rendered_entity;

#[cfg(test)]
mod tests;
