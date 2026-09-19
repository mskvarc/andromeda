//! Federated current-entity query and retrieve operations (clauses 5.7.1, 5.7.2, 6.3.17).

use crate::{
    FederationService,
    batch_query_body::{QuerySelector, build_batch_query_body, selectors_from_query},
    cache::Cacheability,
    config::SplitEntities,
    entity_result::parse_forwarded_entities,
    error::FederationError,
    forwarded_parameter::ForwardedParameter,
    forwarded_query::ForwardedQuery,
    forwarding_url::{build_batch_query_url, build_query_url, build_retrieve_url},
    outcome::{ForwardGetOutcome, ForwardReadOutcome, ForwardedEntities, ForwardedResponse, MatchedSources},
    read_binding::{ReadBinding, ReadIntent, read_binding_for},
    read_fan_out::{ForwardedGetConditions, ForwardedRead, ReadSelection, ReadServing, expand_registration_targets, read_outcome, registered_type_of},
    request::{CsrModeFilter, ForwardQueryRequest, ForwardRetrieveRequest, QueryDelivery},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_jsonld::{compaction::compact_type_iri, context_spec::ContextSpec};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode};
use andromeda_telemetry::federation_ops::{self, FederationOperation};
use chrono::Utc;
use serde_json::Value;
use std::slice::from_ref;
use tracing::warn;
use url::{Url, form_urlencoded};

/// Whether a registration's mode is admitted by a forwarding phase.
pub(crate) fn mode_admitted(registration: &ContextSourceRegistration, mode_filter: CsrModeFilter) -> bool {
    match mode_filter {
        CsrModeFilter::All => true,
        CsrModeFilter::ExcludeAuxiliary => registration.access.mode != Some(ForwardingMode::Auxiliary),
        CsrModeFilter::AuxiliaryOnly => registration.access.mode == Some(ForwardingMode::Auxiliary),
    }
}

/// The one entry a request that already looped back onto this broker
/// contributes: nothing is forwarded, and clause 6.3.17 names the loop with a
/// 199 warning.
pub(crate) fn own_loop_warning() -> NgsiLdWarning {
    NgsiLdWarning::MiscellaneousWarning("registration loop detected: the request's Via chain already names this context broker".to_string())
}

/// The outcome of a current-entity read whose request already looped back
/// onto this broker: no registration is selected, only the warning is reported.
fn own_loop_outcome() -> ForwardReadOutcome<ForwardedEntities> {
    ForwardReadOutcome {
        results: vec![ForwardedEntities {
            registration_id: None,
            mode: ForwardingMode::Inclusive,
            entities: Vec::new(),
            warnings: vec![own_loop_warning()],
            linked_map: None,
        }],
        sources: MatchedSources::default(),
    }
}

impl FederationService {
    /// Read a forwarded GET into the entities it carried, with the clause
    /// 6.3.17 warning for every abnormal outcome. A 404 contributes nothing.
    ///
    /// A query answers with an array and a retrieval with one object, but
    /// either shape is read from either: a retrieval continuing a clause 4.5.23
    /// flat join answers with the entity and its linked entities side by side.
    fn entities_of(registration: &ContextSourceRegistration, outcome: ForwardGetOutcome) -> Option<ForwardedEntities> {
        let mode = registration.access.mode.unwrap_or_default();
        let with_warnings = |entities, warnings, linked_map| {
            Some(ForwardedEntities {
                registration_id: Some(registration.id.clone()), // clone: the result names the registration it came from
                mode,
                entities,
                warnings,
                linked_map,
            })
        };
        match outcome {
            ForwardGetOutcome::Success(ForwardedResponse {
                body: Value::Array(items),
                warnings: cs_warnings,
                entity_map,
            }) => {
                let (entities, parse_warnings) = parse_forwarded_entities(&registration.id, items, Utc::now());
                let mut warnings = cs_warnings;
                warnings.extend(parse_warnings);
                with_warnings(entities, warnings, entity_map)
            }
            ForwardGetOutcome::Success(ForwardedResponse {
                body: object @ Value::Object(_),
                warnings: cs_warnings,
                entity_map,
            }) => {
                let (entities, parse_warnings) = parse_forwarded_entities(&registration.id, vec![object], Utc::now());
                let mut warnings = cs_warnings;
                warnings.extend(parse_warnings);
                with_warnings(entities, warnings, entity_map)
            }
            ForwardGetOutcome::Success(ForwardedResponse {
                body: Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_),
                ..
            }) => {
                warn!("Context source {} returned an unexpected response shape", registration.id);
                with_warnings(
                    Vec::new(),
                    vec![NgsiLdWarning::RevalidationFailed(format!(
                        "Context source {} returned unexpected response type",
                        registration.id
                    ))],
                    None,
                )
            }
            // A 404 is not abnormal (clause 6.3.17), but the warnings the
            // source reported about the sources behind it still travel.
            ForwardGetOutcome::NotFound(cs_warnings) => (!cs_warnings.is_empty()).then(|| with_warnings(Vec::new(), cs_warnings, None)).flatten(),
            other @ (ForwardGetOutcome::InvalidPayload(_)
            | ForwardGetOutcome::ErrorStatus(_)
            | ForwardGetOutcome::Transport(_)
            | ForwardGetOutcome::LoopDetected) => {
                let warning = other.warning(&registration.id)?;
                warn!("Forwarding to context source {} failed: {}", registration.id, warning);
                with_warnings(Vec::new(), vec![warning], None)
            }
        }
    }

    /// Forward an entity query to the matching context sources (clause
    /// 5.7.2.4), each sent the part of the client's query it is registered for.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when the query's entity type
    /// cannot be expanded against the request's context,
    /// [`FederationError::Registration`] when the matching registrations
    /// cannot be read, and [`FederationError::Conflict`] when the selected
    /// registrations are mutually exclusive. A source that fails to answer
    /// does not fail the call: its failure travels as a warning.
    pub async fn forward_query(
        &self,
        target: &ContextTarget,
        request: ForwardQueryRequest<'_>,
    ) -> Result<ForwardReadOutcome<ForwardedEntities>, FederationError> {
        let timer = federation_ops::start(FederationOperation::Query);
        let result = async move {
            if !target.permits_federation() {
                return Ok(ForwardReadOutcome::empty());
            }
            let ForwardQueryRequest {
                client_query,
                delivery,
                split,
                context,
                triggering,
                csf_expr,
                mode_filter,
                entity_map,
                source_page,
            } = request;
            if self.is_loop_detected(triggering.via) {
                return Ok(own_loop_outcome());
            }
            let intent = match delivery {
                QueryDelivery::Query => ReadIntent::Query,
                QueryDelivery::Batch => ReadIntent::QueryBatch,
            };
            let registrations = self
                .select_read_registrations(
                    context,
                    ReadSelection {
                        entity_type: client_query.entity_type(),
                        entity_ids: client_query.entity_ids(),
                        exclude_registration: None,
                        csf: csf_expr,
                        scope_q: client_query.scope_q(),
                        serving: ReadServing::CurrentEntity(intent),
                        mode_filter,
                        entity_map,
                    },
                )
                .await?;
            // Clause 4.3.6.1: each registration is contacted through a read
            // operation it advertises, preferring the binding the client used --
            // a GET query, or a POST batch query. Clause 4.3.6.3: each of its
            // information blocks that covers part of the query is a forwarded
            // request of its own.
            let mut targets = Vec::with_capacity(registrations.len());
            for registration in registrations {
                let Some(binding) = read_binding_for(intent, &registration) else {
                    continue;
                };
                let reads = ForwardedQuery::for_registration_on_page(client_query, &registration, split, source_page)
                    .iter()
                    .map(|query| query_read(&registration, query, binding))
                    .collect::<Vec<_>>();
                expand_registration_targets(registration, reads, &mut targets);
            }
            if targets.is_empty() {
                return Ok(ForwardReadOutcome::empty());
            }
            let matched = targets.len();
            let answers = self
                .fan_out_read(
                    targets,
                    FederationService::entities_of,
                    ForwardedGetConditions {
                        context,
                        triggering,
                        cacheability: Cacheability::Cacheable,
                        entity_map,
                    },
                )
                .await;
            Ok(read_outcome(answers, matched))
        }
        .await;
        timer.finish_result(&result);
        result
    }

    /// Forward an entity retrieval to the matching context sources (clause
    /// 5.7.1.4), each sent the part of the client's parameters it is
    /// registered for.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::Registration`] when the matching
    /// registrations cannot be read, and [`FederationError::Conflict`] when
    /// the selected registrations are mutually exclusive. A source that fails
    /// to answer does not fail the call: its failure travels as a warning.
    pub async fn forward_retrieve(
        &self,
        target: &ContextTarget,
        request: ForwardRetrieveRequest<'_>,
    ) -> Result<ForwardReadOutcome<ForwardedEntities>, FederationError> {
        let timer = federation_ops::start(FederationOperation::Retrieve);
        let result = async move {
            if !target.permits_federation() {
                return Ok(ForwardReadOutcome::empty());
            }
            let ForwardRetrieveRequest {
                entity_id,
                exclude_registration,
                client_query,
                context,
                triggering,
                mode_filter,
                entity_map,
            } = request;
            if self.is_loop_detected(triggering.via) {
                return Ok(own_loop_outcome());
            }
            let registrations = self
                .select_read_registrations(
                    context,
                    ReadSelection {
                        entity_type: None,
                        entity_ids: Some(from_ref(entity_id)),
                        exclude_registration,
                        csf: None,
                        scope_q: None,
                        serving: ReadServing::CurrentEntity(ReadIntent::RetrieveById),
                        mode_filter,
                        entity_map,
                    },
                )
                .await?;
            // A retrieval knows no split entities (clause 5.7.1.4), so every
            // parameter that is not narrowed travels with the request. Clause
            // 4.3.6.1: a registration advertising only queryEntity or only
            // queryBatch is reached through that operation instead of a
            // retrieve-by-id it never registered. Clause 4.3.6.3: each of its
            // blocks covering the entity contributes its own attribute
            // constraint.
            let mut targets = Vec::with_capacity(registrations.len());
            for registration in registrations {
                let Some(binding) = read_binding_for(ReadIntent::RetrieveById, &registration) else {
                    continue;
                };
                let queries = ForwardedQuery::for_registration(client_query, &registration, SplitEntities::Complete);
                if queries.is_empty() {
                    continue;
                }
                // The registered type a retrieve-to-query downgrade names is the
                // same for every block, so it is compacted once (clause 4.3.6.1).
                let compact_type = match binding {
                    ReadBinding::Retrieve => None,
                    ReadBinding::Query | ReadBinding::BatchQuery => self.registration_compact_type(&registration, entity_id, context).await,
                };
                let reads = queries
                    .iter()
                    .map(|query| retrieve_read(registration.access.endpoint.url(), entity_id, query, binding, compact_type.as_deref()))
                    .collect::<Vec<_>>();
                expand_registration_targets(registration, reads, &mut targets);
            }
            if targets.is_empty() {
                return Ok(ForwardReadOutcome::empty());
            }
            let matched = targets.len();
            let answers = self
                .fan_out_read(
                    targets,
                    FederationService::entities_of,
                    ForwardedGetConditions {
                        context,
                        triggering,
                        cacheability: Cacheability::Cacheable,
                        entity_map,
                    },
                )
                .await;
            Ok(read_outcome(answers, matched))
        }
        .await;
        timer.finish_result(&result);
        result
    }

    /// The compact type a registration covers `entity_id` under, for a forwarded
    /// query or batch query that names the entity by id (clause 4.3.6.1). Stored
    /// registration types are expanded IRIs, so the covered type is compacted
    /// against the request context to the term a source recognises.
    async fn registration_compact_type(&self, registration: &ContextSourceRegistration, entity_id: &Uri, context: &ContextSpec) -> Option<String> {
        let registered = registered_type_of(from_ref(registration), entity_id)?;
        Some(compact_type_iri(&self.engine, registered.as_ref(), context).await)
    }
}

/// The forwarded read one registration receives for a retrieve-by-id, from the
/// operation it advertises (clause 4.3.6.1): a direct retrieve, a GET query
/// naming the entity by id and its registered type, or a POST batch query with
/// the same selector. The registered type is compacted once by the caller and
/// passed in, since it is the same for every block of the registration.
fn retrieve_read(endpoint: &Url, entity_id: &Uri, query: &ForwardedQuery, binding: ReadBinding, compact_type: Option<&str>) -> ForwardedRead {
    match binding {
        ReadBinding::Retrieve => ForwardedRead::Get(build_retrieve_url(endpoint, entity_id, &query.to_query_string())),
        ReadBinding::Query => ForwardedRead::Get(build_query_url(endpoint, &retrieve_query_string(entity_id, compact_type, query))),
        ReadBinding::BatchQuery => {
            let selectors = [QuerySelector {
                entity_type: compact_type.map(str::to_string),
                id: Some(entity_id.as_str().to_string()),
            }];
            let body = build_batch_query_body(&selectors, query.value_of(ForwardedParameter::Attrs));
            ForwardedRead::PostQuery {
                url: build_batch_query_url(endpoint),
                body,
            }
        }
    }
}

/// The forwarded read one registration receives for a query, from the operation
/// it advertises (clause 4.3.6.1): a GET query, or a POST batch query when it
/// advertises only queryBatch. A query is never bound to a retrieve, so that
/// case degrades to the GET query it would otherwise be.
fn query_read(registration: &ContextSourceRegistration, query: &ForwardedQuery, binding: ReadBinding) -> ForwardedRead {
    let endpoint = registration.access.endpoint.url();
    match binding {
        ReadBinding::Query | ReadBinding::Retrieve => ForwardedRead::Get(build_query_url(endpoint, &query.to_query_string())),
        ReadBinding::BatchQuery => {
            let selectors = selectors_from_query(query);
            let body = build_batch_query_body(&selectors, query.value_of(ForwardedParameter::Attrs));
            ForwardedRead::PostQuery {
                url: build_batch_query_url(endpoint),
                body,
            }
        }
    }
}

/// The GET query string a retrieve-by-id is downgraded to (clause 4.3.6.1): the
/// entity named by id and its registered type, followed by the parameters the
/// retrieval narrowing left (attributes, `datasetId`, language).
fn retrieve_query_string(entity_id: &Uri, compact_type: Option<&str>, query: &ForwardedQuery) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    serializer.append_pair("id", entity_id.as_str());
    if let Some(entity_type) = compact_type {
        serializer.append_pair("type", entity_type);
    }
    let selection = serializer.finish();
    let narrowed = query.to_query_string();
    if narrowed.is_empty() { selection } else { format!("{selection}&{narrowed}") }
}
