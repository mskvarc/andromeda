//! Temporal retrieval and query execution policy.

use crate::{
    aggregation_eligibility::{disqualifying_shapes, ensure_aggregation_eligible},
    error::TemporalError,
    port::{
        history_reader::{ShapeHolders, TemporalHistoryReader},
        read_scope::{ScopeReach, TemporalReadScope},
    },
    query_reading::QueryReading,
    read::{dataset_step::DatasetStep, entity_members::carries_members, rendered_entity::HistoryPresence},
    requested_identifiers::RequestedIdentifiers,
    service::{TemporalReadResult, TemporalService},
    temporal_invariant::TemporalInvariant,
    temporal_not_found_reason::TemporalNotFoundReason,
    temporal_operation_unsupported::TemporalOperationUnsupported,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_compaction::entity_compactor::EntityCompactor;
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_query::{
    candidate_filters::CandidateFilters,
    count_policy::CountPolicy,
    page_window::PageWindow,
    temporal::compiled::CompiledTemporalQuery,
};
use andromeda_ngsi_ld_request::query::temporal::{TemporalQueryParameters, operation::TemporalQueryOperation};
use andromeda_telemetry::temporal::{self, TemporalOperation};
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
            params.validate(TemporalQueryOperation::Retrieve, self.linked_retrieval)?;

            let compiled = self.compile_temporal_query(params, context, self.language_targeting()).await?;
            ensure_aggregation_supported(self.reader.as_ref(), &compiled.representation.aggr_methods)?;
            if shapes_decide_eligibility(&compiled) {
                let every = self.reader.read_scope(target, ScopeReach::Every).await?;
                self.ensure_requested_attributes_aggregable(&every, &compiled, ShapeHolders::Entity(entity_id))
                    .await?;
            }
            let scope = self.reader.read_scope(target, ScopeReach::Entity(entity_id)).await?;

            // Clauses 4.5.1 and 4.5.6: the temporal catalogue is the authority for
            // an entity's types, so the mandatory `type` member survives the clause
            // 5.6.6 deletion that removes current state and leaves the Temporal
            // Evolution standing; an entity it records no evolution of is absent.
            let Some(entity_types) = self.reader.entity_types(&scope, from_ref(entity_id)).await?.remove(entity_id) else {
                // clone: the refusal owns the identifier it names.
                return Err(TemporalError::NotFound(TemporalNotFoundReason::Entity { id: entity_id.clone() }));
            };

            let compactor = self.engine.compactor(context);
            let rendered = self
                .render_one_temporal_entity(&scope, entity_id, &entity_types, &compiled, &EntityCompactor::new(&compactor))
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
            if restricted && self.window_presence(&scope, entity_id, &compiled, rendered.presence).await? == HistoryPresence::Absent {
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
            if projected && !carries_members(&rendered.history, &compiled.projection) {
                return Err(TemporalError::NotFound(TemporalNotFoundReason::ProjectionLeftNothing {
                    entity: entity_id.clone(),
                }));
            }

            // Clause 6.3.10: a page that left instances out is a 206 carrying the
            // `Content-Range` its client resumes from.
            let range = rendered.cut.map(|cut| cut.range(compiled.window.interval, rendered.time_range));

            Ok(TemporalReadResult {
                history: rendered.history,
                range,
                projection: compiled.projection,
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
        scope: &TemporalReadScope<'_>,
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
        Ok(self.read_history_page(scope, entity_id, &window_only).await?.presence())
    }

    /// Execute stage 1 entity ID resolution over the page `window` selects,
    /// without fetching temporal history.
    ///
    /// Used to build `EntityMaps` (clause 5.5.14). Runs only the lightweight
    /// stage 1 statement (entity ID resolution). A map fixes the whole result
    /// set and is read with [`PageWindow::unbounded`], whatever page the
    /// request names. `filters` decides whether the value, geo and scope
    /// filters hold the listed entities (clause 5.14.5.4).
    ///
    /// The request's terms and `q` target languages are read as `reading`
    /// says.
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
        window: &PageWindow,
        reading: QueryReading<'_>,
        identifiers: RequestedIdentifiers,
        filters: CandidateFilters,
    ) -> Result<Vec<Identifier>, TemporalError> {
        let QueryReading { context, language_targeting } = reading;
        let timer = temporal::start(TemporalOperation::QueryIds);
        let result = async {
            params.validate(TemporalQueryOperation::Query, self.linked_retrieval)?;
            let mut compiled = self.compile_temporal_query(params, context, language_targeting).await?;
            match identifiers {
                RequestedIdentifiers::Narrow => {}
                RequestedIdentifiers::Ignore => compiled.entities.without_identifier_narrowing(),
            }
            match filters {
                CandidateFilters::Applied => {}
                CandidateFilters::Deferred => compiled.entities.without_split_filters(),
            }
            let compiled = compiled;
            ensure_aggregation_supported(self.reader.as_ref(), &compiled.representation.aggr_methods)?;
            let scope = self.reader.read_scope(target, ScopeReach::Every).await?;
            let outcome = self.reader.query_entity_ids(&scope, &compiled, window, CountPolicy::Omit).await?;
            Ok(outcome.entity_ids)
        }
        .await;
        timer.finish_result(&result);
        result
    }
}

impl TemporalService {
    /// Clause 5.7.4.4: an aggregated representation over an attribute that is
    /// not eligible for one of the requested methods is an `InvalidRequest`,
    /// decided before any history is read for the response from the families
    /// and value shapes of the instances it would aggregate: those of the
    /// Entities `holders` names, inside the window and of the datasets the
    /// request names. The shapes the attribute holds anywhere in the branch
    /// are read first, and only those some method refuses are looked for among
    /// the instances. Only the attributes the request named can be checked
    /// ahead of the read; a request over every attribute is not, which is why
    /// the statement itself must also refuse to fold a value-derived aggregate
    /// for a family Table 4.5.19.1-3 marks N/A.
    async fn ensure_requested_attributes_aggregable(
        &self,
        scope: &TemporalReadScope<'_>,
        compiled: &CompiledTemporalQuery,
        holders: ShapeHolders<'_>,
    ) -> Result<(), TemporalError> {
        if !shapes_decide_eligibility(compiled) {
            return Ok(());
        }
        let attributes = compiled
            .instances
            .attr_filter
            .iter()
            .map(|iri| iri.as_str().parse::<AttributeName>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| TemporalError::from(TemporalInvariant::ExpandedAttributeName(error)))?;
        let methods = &compiled.representation.aggr_methods;
        let recorded = self.reader.recorded_attribute_shapes(scope, &attributes).await?;
        let disqualifying = disqualifying_shapes(methods, &recorded);
        let held = if disqualifying.is_empty() {
            HashMap::new()
        } else {
            self.reader.held_attribute_shapes(scope, holders, compiled, &disqualifying).await?
        };
        ensure_aggregation_eligible(&attributes, methods, &recorded, &held).map_err(TemporalError::from)
    }
}

/// Whether the shapes the requested attributes were recorded with decide the
/// request's eligibility before its history is read: an aggregated
/// representation naming its attributes.
const fn shapes_decide_eligibility(compiled: &CompiledTemporalQuery) -> bool {
    compiled.representation.format.is_aggregated() && !compiled.instances.attr_filter.is_empty()
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

/// Reading the aggregated periods of a response and cutting them together.
mod aggregated_page;
/// Compacting the histories one read gathered under its request context.
pub(crate) mod compacted_history;
/// Whether a temporal read carries the `datasetId` step.
mod dataset_step;
/// Whether a projection leaves a history an Entity member.
mod entity_members;
/// Rendering the contributed entities of a split query over their merged instances.
pub mod merged_rendering;
/// Query Temporal Evolution of Entities (clause 5.7.4).
mod query;
/// Rendering the temporal evolution of one entity over a compiled window.
pub(crate) mod rendered_entity;
/// The cut a partial response is made at, and the request that continues it.
mod response_cut;
/// An entity of a page whose stored history did not read back.
mod unread_entity;

/// What a listing of the identifiers a temporal query matches holds them to.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod candidate_listing_tests;
/// Whose instances decide the aggregation eligibility of a temporal read.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod eligibility_tests;
#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
