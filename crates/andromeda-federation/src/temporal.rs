//! Federated temporal entity query and retrieve operations (clauses 5.7.3, 5.7.4, 6.3.17).

use crate::{
    FederationService,
    cache::Cacheability,
    config::SplitEntities,
    entity_read::own_loop_warning,
    error::FederationError,
    forwarded_query::{ForwardedQuery, client_query::ClientQuery},
    forwarding_url::{build_temporal_query_url, build_temporal_retrieve_url},
    operation::ForwardingOperation,
    outcome::{ForwardGetOutcome, ForwardReadOutcome, ForwardedResponse, ForwardedTemporalEntities, MatchedSources},
    query_forwarding_error::QueryForwardingError,
    read_fan_out::{ForwardedGetConditions, ForwardedRead, ReadSelection, ReadServing, expand_registration_targets, read_outcome},
    request::{ForwardRetrieveRequest, ForwardTemporalQueryRequest, SourcePaging},
    source_walk::WalkedAnswer,
    term_resolver::TermResolver,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_http_headers::{ngsild_content_range::NgsiLdContentRange, ngsild_warning::NgsiLdWarning};
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode};
use serde_json::Value;
use std::slice::from_ref;
use tracing::warn;

/// A Query Temporal Evolution of Entities answer walks page by page as the
/// temporal entities each page carried.
impl WalkedAnswer for ForwardedTemporalEntities {
    type Element = Value;

    fn read(registration: &ContextSourceRegistration, outcome: ForwardGetOutcome) -> Option<ForwardedTemporalEntities> {
        FederationService::temporal_entities_of(registration, outcome)
    }

    fn elements(&mut self) -> &mut Vec<Value> {
        &mut self.entities
    }

    fn warnings(&mut self) -> &mut Vec<NgsiLdWarning> {
        &mut self.warnings
    }

    fn identifier(element: &Value) -> Option<&str> {
        element.get("id").and_then(Value::as_str)
    }

    fn absorb(&mut self, page: ForwardedTemporalEntities) {
        self.warnings.extend(page.warnings);
        self.partial_ranges.extend(page.partial_ranges);
    }
}

/// The outcome of a temporal read whose request already looped back onto this
/// broker: no registration is selected, only the warning is reported.
fn own_loop_outcome() -> ForwardReadOutcome<ForwardedTemporalEntities> {
    ForwardReadOutcome {
        results: vec![ForwardedTemporalEntities {
            registration_id: None,
            mode: ForwardingMode::Inclusive,
            entities: Vec::new(),
            warnings: vec![own_loop_warning()],
            linked_map: None,
            partial_ranges: Vec::new(),
        }],
        sources: MatchedSources::default(),
    }
}

impl FederationService {
    /// Read a forwarded temporal GET, with the clause 6.3.17 warning for every
    /// abnormal outcome. A 404 contributes nothing. A query answers with an
    /// array and a retrieval with one object, but either is read from either:
    /// a retrieval continuing a clause 4.5.23 flat join answers with an array.
    fn temporal_entities_of(registration: &ContextSourceRegistration, outcome: ForwardGetOutcome) -> Option<ForwardedTemporalEntities> {
        let mode = registration.access.mode.unwrap_or_default();
        let answered = |entities, warnings, linked_map, partial_range: Option<NgsiLdContentRange>| {
            Some(ForwardedTemporalEntities {
                registration_id: Some(registration.id.clone()), // clone: the result names the registration it came from
                mode,
                entities,
                warnings,
                linked_map,
                partial_ranges: partial_range.into_iter().collect(),
            })
        };
        match outcome {
            ForwardGetOutcome::Success(ForwardedResponse {
                body: Value::Array(items),
                warnings,
                entity_map,
                partial_range,
                ..
            }) => answered(items, warnings, entity_map, partial_range),
            ForwardGetOutcome::Success(ForwardedResponse {
                body: object @ Value::Object(_),
                warnings,
                entity_map,
                partial_range,
                ..
            }) => answered(vec![object], warnings, entity_map, partial_range),
            ForwardGetOutcome::Success(ForwardedResponse {
                body: Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_),
                ..
            }) => {
                warn!("Context source {} returned an unexpected temporal response shape", registration.id);
                answered(
                    Vec::new(),
                    vec![NgsiLdWarning::RevalidationFailed(format!(
                        "Context source {} returned unexpected response type",
                        registration.id
                    ))],
                    None,
                    None,
                )
            }
            // A 404 is not abnormal (clause 6.3.17), but the warnings the
            // source reported about the sources behind it still travel.
            ForwardGetOutcome::NotFound(cs_warnings) => (!cs_warnings.is_empty()).then(|| answered(Vec::new(), cs_warnings, None, None)).flatten(),
            other @ (ForwardGetOutcome::InvalidPayload(_)
            | ForwardGetOutcome::ErrorStatus(_)
            | ForwardGetOutcome::Transport(_)
            | ForwardGetOutcome::LoopDetected) => {
                let warning = other.warning(&registration.id)?;
                warn!("Temporal forwarding to context source {} failed: {}", registration.id, warning);
                answered(Vec::new(), vec![warning], None, None)
            }
        }
    }

    /// Forward a temporal entity query to the matching registrations (clause
    /// 5.7.4.4), each sent the part of the client's query it is registered for,
    /// and read as much of each source's answer as `paging` asks for: every
    /// page it links with `next` (clauses 4.3.6.7 and 5.5.9.1), or one
    /// explicit page.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when the query's entity type
    /// cannot be expanded against the request's context,
    /// [`FederationError::Registration`] when the matching registrations
    /// cannot be read, and [`FederationError::Conflict`] when the selected
    /// registrations are mutually exclusive, each as
    /// [`QueryForwardingError::Federation`], and
    /// [`QueryForwardingError::TooManyResults`] when a walk's answers exceed
    /// the request's budget (clause 4.12). A source that fails to answer does
    /// not fail the call: its failure travels as a warning.
    pub async fn forward_temporal_query(
        &self,
        target: &ContextTarget,
        request: ForwardTemporalQueryRequest<'_>,
    ) -> Result<ForwardReadOutcome<ForwardedTemporalEntities>, QueryForwardingError> {
        if !target.permits_federation() {
            return Ok(ForwardReadOutcome::empty());
        }
        let ForwardTemporalQueryRequest {
            client_query,
            split,
            context,
            operation,
            triggering,
            csf_expr,
            mode_filter,
            entity_map,
            paging,
        } = request;
        if self.is_loop_detected(triggering.via) {
            return Ok(own_loop_outcome());
        }
        let client = ClientQuery::Temporal(client_query);
        let registrations = self
            .select_read_registrations(
                context,
                ReadSelection {
                    entity_type: client.entity_type(),
                    entity_ids: client.entity_ids(),
                    exclude_registration: None,
                    csf: csf_expr,
                    scope_q: client_query.scope_q.as_ref(),
                    serving: ReadServing::Operation(operation),
                    mode_filter,
                    entity_map,
                },
            )
            .await?;
        // Clause 4.3.6.3: each information block that covers part of the query
        // is a forwarded request of its own.
        let mut targets = Vec::with_capacity(registrations.len());
        let resolver = TermResolver::new(&self.engine, context);
        for registration in registrations {
            let queries = ForwardedQuery::for_registration_on_page(client, &registration, split, paging.explicit_page(), &resolver).await?;
            let reads = queries
                .iter()
                .map(|query| ForwardedRead::Get(build_temporal_query_url(registration.access.endpoint.url(), &query.to_query_string())))
                .collect();
            expand_registration_targets(registration, reads, &mut targets);
        }
        if targets.is_empty() {
            return Ok(ForwardReadOutcome::empty());
        }
        let matched = targets.len();
        // Clause 6.3.17 allows caching for `/entities` only.
        let conditions = ForwardedGetConditions {
            context,
            triggering,
            cacheability: Cacheability::Uncacheable,
            entity_map,
        };
        let answers = match paging {
            SourcePaging::Walk(budget) => self.fan_out_walk(targets, conditions, budget).await?,
            SourcePaging::Page(_) => self.fan_out_read(targets, FederationService::temporal_entities_of, conditions).await,
        };
        Ok(read_outcome(answers, matched))
    }

    /// Forward a temporal entity retrieval to the matching context sources
    /// (clause 5.7.3.4), each sent the part of the client's parameters it is
    /// registered for.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::Registration`] when the matching
    /// registrations cannot be read, and [`FederationError::Conflict`] when
    /// the selected registrations are mutually exclusive. A source that fails
    /// to answer does not fail the call: its failure travels as a warning.
    pub async fn forward_temporal_retrieve(
        &self,
        target: &ContextTarget,
        request: ForwardRetrieveRequest<'_>,
    ) -> Result<ForwardReadOutcome<ForwardedTemporalEntities>, FederationError> {
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
                    serving: ReadServing::Operation(ForwardingOperation::RetrieveTemporalEntity),
                    mode_filter,
                    entity_map,
                },
            )
            .await?;
        // A retrieval knows no split entities (clause 5.7.3.4); clause 4.3.6.3:
        // each block covering the entity contributes its own attribute
        // constraint.
        let mut targets = Vec::with_capacity(registrations.len());
        let resolver = TermResolver::new(&self.engine, context);
        for registration in registrations {
            let queries = ForwardedQuery::for_registration(client_query, &registration, SplitEntities::Complete, &resolver).await?;
            expand_registration_targets(registration, queries, &mut targets);
        }
        if targets.is_empty() {
            return Ok(ForwardReadOutcome::empty());
        }
        let matched = targets.len();
        let answers = self
            .fan_out_get(
                targets,
                |registration, query| build_temporal_retrieve_url(registration.access.endpoint.url(), entity_id, &query.to_query_string()),
                FederationService::temporal_entities_of,
                ForwardedGetConditions {
                    context,
                    triggering,
                    cacheability: Cacheability::Uncacheable,
                    entity_map,
                },
            )
            .await;
        Ok(read_outcome(answers, matched))
    }
}
