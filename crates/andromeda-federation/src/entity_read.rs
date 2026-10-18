//! Federated current-entity query and retrieve operations (clauses 5.7.1, 5.7.2, 6.3.17).

use crate::{
    FederationService,
    answer_reading::{ReceivedAnswer, received_answer},
    cache::Cacheability,
    config::SplitEntities,
    error::FederationError,
    forwarded_batch_query::batch_query,
    forwarded_parameter::ForwardedParameter,
    forwarded_query::ForwardedQuery,
    forwarding_context::BodyPresence,
    forwarding_url::{build_batch_query_url, build_query_url, build_retrieve_url},
    operation::ForwardingOperation,
    outcome::{ForwardReadOutcome, ForwardedEntities, MatchedSources},
    query_forwarding_error::QueryForwardingError,
    read_binding::{ReadBinding, ReadIntent, read_binding_for},
    read_fan_out::{ForwardedGetConditions, ForwardedRead, ReadSelection, ReadServing, expand_registration_targets, read_outcome},
    registration_narrowing::Narrowing,
    request::{CsrModeFilter, ForwardQueryRequest, ForwardRetrieveRequest, QueryDelivery},
    source_walk::SOURCE_PAGE_SIZE,
    term_resolver::TermResolver,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode};
use andromeda_telemetry::federation_ops::{self, FederationOperation};
use std::slice::from_ref;

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
    /// Forward an entity query to the matching context sources (clause
    /// 5.7.2.4), each sent the part of the client's query it is registered
    /// for, and gather every page of each source's answer (clauses 4.3.6.7,
    /// 5.5.9.1).
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when the query's entity type
    /// cannot be expanded against the request's context,
    /// [`FederationError::Registration`] when the matching registrations
    /// cannot be read, and [`FederationError::Conflict`] when the selected
    /// registrations are mutually exclusive, each as
    /// [`QueryForwardingError::Federation`], and
    /// [`QueryForwardingError::TooManyResults`] when the sources' answers
    /// exceed the request's budget. A source that fails to answer does not
    /// fail the call: its failure travels as a warning.
    pub async fn forward_query(
        &self,
        target: &ContextTarget,
        request: ForwardQueryRequest<'_>,
    ) -> Result<ForwardReadOutcome<ForwardedEntities>, QueryForwardingError> {
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
                geo,
                mode_filter,
                entity_map,
                budget,
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
                        id_pattern: client_query.id_pattern(),
                        exclude_registration: None,
                        csf: csf_expr,
                        scope_q: client_query.scope_q(),
                        geo,
                        serving: ReadServing::EntityQuery(intent),
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
            let resolver = TermResolver::new(&self.engine, context);
            for registration in registrations {
                let Some(binding) = read_binding_for(intent, &registration) else {
                    continue;
                };
                // Clause 6.23 carries a batch query in its body, whose media
                // type decides how its context travels (clause 6.3.5).
                let presence = match binding {
                    ReadBinding::Query => BodyPresence::Absent,
                    ReadBinding::BatchQuery => BodyPresence::Present,
                };
                let narrowing = Narrowing::builder()
                    .client(client_query)
                    .inbound(context)
                    .resolver(&resolver)
                    .split(split)
                    .presence(presence)
                    .build();
                let reads = self
                    .registration_queries(&registration, &narrowing)
                    .await?
                    .into_reads(|query| query_read(&registration, query, binding));
                expand_registration_targets(registration, reads, &mut targets);
            }
            if targets.is_empty() {
                return Ok(ForwardReadOutcome::empty());
            }
            let matched = targets.len();
            let answers = self
                .fan_out_walk::<ReceivedAnswer>(
                    targets,
                    ForwardedGetConditions {
                        context,
                        triggering,
                        cacheability: Cacheability::Cacheable,
                        entity_map,
                    },
                    budget,
                )
                .await?;
            Ok(self.answered_states(read_outcome(answers, matched)).await)
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
                        entity_type: client_query.entity_type(),
                        entity_ids: Some(from_ref(entity_id)),
                        id_pattern: None,
                        exclude_registration,
                        csf: None,
                        scope_q: None,
                        geo: None,
                        // Clause 5.7.1.4: a retrieval is forwarded only to the
                        // registrations that "match and support the
                        // retrieveEntity operation" (Table 4.20-1); one
                        // advertising only a query operation does not apply.
                        serving: ReadServing::Operation(ForwardingOperation::RetrieveEntity),
                        mode_filter,
                        entity_map,
                    },
                )
                .await?;
            // A retrieval knows no split entities (clause 5.7.1.4), so every
            // parameter that is not narrowed travels with the request. Clause
            // 4.3.6.3: each of its blocks covering the entity contributes its
            // own attribute constraint.
            let mut targets = Vec::with_capacity(registrations.len());
            let resolver = TermResolver::new(&self.engine, context);
            let narrowing = Narrowing::builder()
                .client(client_query)
                .inbound(context)
                .resolver(&resolver)
                .split(SplitEntities::Complete)
                .presence(BodyPresence::Absent)
                .build();
            for registration in registrations {
                let reads = self
                    .registration_queries(&registration, &narrowing)
                    .await?
                    .into_reads(|query| ForwardedRead::Get(build_retrieve_url(registration.access.endpoint.url(), entity_id, &query.to_query_string())));
                expand_registration_targets(registration, reads, &mut targets);
            }
            if targets.is_empty() {
                return Ok(ForwardReadOutcome::empty());
            }
            let matched = targets.len();
            let answers = self
                .fan_out_read(
                    targets,
                    |registration, outcome| received_answer(registration, outcome).answer,
                    ForwardedGetConditions {
                        context,
                        triggering,
                        cacheability: Cacheability::Cacheable,
                        entity_map,
                    },
                )
                .await;
            Ok(self.answered_states(read_outcome(answers, matched)).await)
        }
        .await;
        timer.finish_result(&result);
        result
    }
}

/// The forwarded read one registration receives for a query, through the
/// binding it advertises (clauses 4.3.6.1, 6.23.1): a GET query, or a POST
/// batch query.
fn query_read(registration: &ContextSourceRegistration, query: &ForwardedQuery, binding: ReadBinding) -> ForwardedRead {
    let endpoint = registration.access.endpoint.url();
    match binding {
        ReadBinding::Query => ForwardedRead::Get(build_query_url(endpoint, &query.to_query_string())),
        ReadBinding::BatchQuery => {
            // Clause 6.23 carries the query in the body; the page size travels
            // as the URI parameter clause 6.3.10 defines for it.
            let page = format!("{}={}", ForwardedParameter::Limit, SOURCE_PAGE_SIZE);
            match batch_query(query) {
                Ok(body) => ForwardedRead::PostQuery {
                    url: build_batch_query_url(endpoint, &page),
                    body: Box::new(body),
                },
                // A query that cannot be written is the registration's failure
                // to prepare, which a read reports as its own.
                Err(error) => ForwardedRead::Get(Err(error)),
            }
        }
    }
}
