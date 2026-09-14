//! Federated current-entity query and retrieve operations (clauses 5.7.1, 5.7.2, 6.3.17).

use crate::{
    FederationService,
    cache::Cacheability,
    config::SplitEntities,
    entity_result::parse_forwarded_entities,
    error::FederationError,
    forwarded_query::ForwardedQuery,
    forwarding_url::{build_query_url, build_retrieve_url},
    outcome::{ForwardGetOutcome, ForwardReadOutcome, ForwardedEntities, ForwardedResponse, MatchedSources},
    read_fan_out::{ForwardedGetConditions, ReadSelection, read_outcome, registered_type_of},
    request::{CsrModeFilter, ForwardQueryRequest, ForwardRetrieveRequest},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode};
use andromeda_telemetry::federation_ops::{self, FederationOperation};
use chrono::Utc;
use serde_json::Value;
use std::slice::from_ref;
use tracing::warn;

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
                split,
                context,
                operation,
                triggering,
                csf_expr,
                mode_filter,
                entity_map,
                source_page,
            } = request;
            if self.is_loop_detected(triggering.via) {
                return Ok(own_loop_outcome());
            }
            let registrations = self
                .select_read_registrations(
                    context,
                    ReadSelection {
                        entity_type: client_query.entity_type(),
                        entity_ids: client_query.entity_ids(),
                        exclude_registration: None,
                        csf: csf_expr,
                        scope_q: client_query.scope_q(),
                        operation,
                        mode_filter,
                        entity_map,
                    },
                )
                .await?;
            let targets = registrations
                .into_iter()
                .filter_map(|registration| {
                    ForwardedQuery::for_registration_on_page(client_query, &registration, split, source_page).map(|query| (registration, query))
                })
                .collect::<Vec<_>>();
            if targets.is_empty() {
                return Ok(ForwardReadOutcome::empty());
            }
            let matched = targets.len();
            let answers = self
                .fan_out_get(
                    targets,
                    |registration, query| build_query_url(registration.access.endpoint.url(), &query.to_query_string()),
                    FederationService::entities_of,
                    ForwardedGetConditions {
                        context,
                        triggering,
                        cacheability: Cacheability::Cacheable,
                        entity_map,
                    },
                )
                .await;
            Ok(read_outcome(answers, matched, None))
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
                operation,
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
                        operation,
                        mode_filter,
                        entity_map,
                    },
                )
                .await?;
            let registered_type = registered_type_of(&registrations, entity_id);
            // A retrieval knows no split entities (clause 5.7.1.4), so every
            // parameter that is not narrowed travels with the request.
            let targets = registrations
                .into_iter()
                .filter_map(|registration| {
                    ForwardedQuery::for_registration(client_query, &registration, SplitEntities::Complete).map(|query| (registration, query))
                })
                .collect::<Vec<_>>();
            if targets.is_empty() {
                return Ok(ForwardReadOutcome::empty());
            }
            let matched = targets.len();
            let answers = self
                .fan_out_get(
                    targets,
                    |registration, query| build_retrieve_url(registration.access.endpoint.url(), entity_id, &query.to_query_string()),
                    FederationService::entities_of,
                    ForwardedGetConditions {
                        context,
                        triggering,
                        cacheability: Cacheability::Cacheable,
                        entity_map,
                    },
                )
                .await;
            Ok(read_outcome(answers, matched, registered_type))
        }
        .await;
        timer.finish_result(&result);
        result
    }
}
