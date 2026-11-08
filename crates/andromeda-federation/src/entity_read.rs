//! Federated current-entity query and retrieve operations (clauses 5.7.1, 5.7.2, 6.3.17).

use crate::{
    FederationService,
    answer_reading::{ReceivedAnswer, received_answer},
    cache::Cacheability,
    error::FederationError,
    forwarded_batch_query::batch_query,
    forwarded_parameter::ForwardedParameter,
    forwarded_query::ForwardedQuery,
    forwarding_context::BodyPresence,
    forwarding_url::{build_batch_query_url, build_query_url, build_retrieve_url},
    matched_registrations::MatchedRegistrations,
    operation::ForwardingOperation,
    outcome::{ForwardReadOutcome, ForwardedEntities, MatchedSources},
    query_forwarding_error::QueryForwardingError,
    read_binding::{ReadBinding, ReadIntent, read_binding_for},
    read_fan_out::{ForwardedGetConditions, ForwardedRead, expand_registration_targets, read_outcome},
    registration_narrowing::{Narrowing, get_reads},
    registration_selection::{RegistrationSelection, RegistrationServing},
    request::{CsrModeFilter, ForwardQueryRequest, ForwardRetrieveRequest, QueryDelivery},
    retrieval_answer::{document, settle, shape_of},
    source_reach::SourceReach,
    source_walk::SOURCE_PAGE_SIZE,
    split_entities::SplitEntities,
    term_resolver::TermResolver,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode};
use andromeda_telemetry::federation_ops::{self, FederationOperation};

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
    NgsiLdWarning::new(
        WarnCode::MiscellaneousWarning,
        "registration loop detected: the request's Via chain already names this context broker",
    )
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
    /// Read the registrations an entity query matches (clause 5.12), whatever
    /// their mode, for the phases of the request to contact. A request that
    /// cannot be forwarded, because the target admits no federation or the
    /// request already looped back onto this broker, matches none and reads
    /// nothing.
    ///
    /// # Errors
    ///
    /// As [`FederationService::forward_query`] for the selection of
    /// registrations.
    pub async fn match_query(&self, target: &ContextTarget, request: &ForwardQueryRequest<'_>) -> Result<MatchedRegistrations, QueryForwardingError> {
        if !target.permits_federation() || self.is_loop_detected(request.triggering.via) {
            return Ok(MatchedRegistrations::new(Vec::new()));
        }
        let registrations = self
            .matching_registrations(
                request.context,
                RegistrationSelection {
                    selectors: &request.client_query.selectors(),
                    reach: SourceReach::Every,
                    csf: request.csf_expr,
                    scope_q: request.split.registration_scope_query(request.client_query.scope_q()),
                    geo: request.geo,
                    serving: RegistrationServing::EntityQuery(read_intent(request.delivery)),
                    mode_filter: CsrModeFilter::All,
                    entity_map: request.entity_map,
                },
            )
            .await?;
        Ok(MatchedRegistrations::new(registrations))
    }

    /// Forward an entity query to `registrations`, the ones a phase of the
    /// request contacts (clause 5.7.2.4), each sent the part of the client's
    /// query it is registered for, and gather every page of each source's
    /// answer (clauses 4.3.6.7, 5.5.9.1).
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when the query's entity type
    /// cannot be expanded against the request's context, and
    /// [`FederationError::Conflict`] when the registrations are mutually
    /// exclusive, each as [`QueryForwardingError::Federation`], and
    /// [`QueryForwardingError::TooManyResults`] when the sources' answers
    /// exceed the request's budget. A source that fails to answer does not
    /// fail the call: its failure travels as a warning.
    pub async fn forward_query(
        &self,
        target: &ContextTarget,
        request: ForwardQueryRequest<'_>,
        registrations: Vec<ContextSourceRegistration>,
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
                csf_expr: _,
                geo: _,
                entity_map,
                budget,
            } = request;
            if self.is_loop_detected(triggering.via) {
                return Ok(own_loop_outcome());
            }
            let intent = read_intent(delivery);
            // Clause 4.3.6.1: each registration is contacted through a read
            // operation it advertises, preferring the binding the client used:
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

    /// Read the registrations an entity retrieval matches (clause 5.12), in
    /// the modes the request contacts, for its phases to split. A
    /// request that cannot be forwarded, because the target admits no
    /// federation or the request already looped back onto this broker,
    /// matches none and reads nothing.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::Registration`] when the matching
    /// registrations cannot be read.
    pub async fn match_retrieve(&self, target: &ContextTarget, request: &ForwardRetrieveRequest<'_>) -> Result<MatchedRegistrations, FederationError> {
        if !target.permits_federation() || self.is_loop_detected(request.triggering.via) {
            return Ok(MatchedRegistrations::new(Vec::new()));
        }
        let registrations = self
            .select_registrations(
                request.context,
                RegistrationSelection {
                    selectors: &request.client_query.selectors(),
                    reach: request.reach,
                    csf: None,
                    scope_q: None,
                    geo: None,
                    // Clause 5.7.1.4: a retrieval is forwarded only to the
                    // registrations that "match and support the
                    // retrieveEntity operation" (Table 4.20-1); one
                    // advertising only a query operation does not apply.
                    serving: RegistrationServing::Operation(ForwardingOperation::RetrieveEntity),
                    mode_filter: request.mode_filter,
                    entity_map: request.entity_map,
                },
            )
            .await?;
        Ok(MatchedRegistrations::new(registrations))
    }

    /// Forward an entity retrieval to `registrations`, the ones a phase of the
    /// request contacts (clause 5.7.1.4), each sent the part of the client's
    /// parameters it is registered for.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::Conflict`] when the registrations are
    /// mutually exclusive. A source that fails to answer does not fail the
    /// call: its failure travels as a warning.
    pub async fn forward_retrieve(
        &self,
        target: &ContextTarget,
        request: ForwardRetrieveRequest<'_>,
        registrations: Vec<ContextSourceRegistration>,
    ) -> Result<ForwardReadOutcome<ForwardedEntities>, FederationError> {
        let timer = federation_ops::start(FederationOperation::Retrieve);
        let result = async move {
            if !target.permits_federation() {
                return Ok(ForwardReadOutcome::empty());
            }
            let ForwardRetrieveRequest {
                entity_id,
                reach: _,
                client_query,
                context,
                triggering,
                mode_filter: _,
                entity_map,
            } = request;
            if self.is_loop_detected(triggering.via) {
                return Ok(own_loop_outcome());
            }
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
                let reads = self.registration_queries(&registration, &narrowing).await?.into_reads(|query| {
                    vec![ForwardedRead::Get(build_retrieve_url(
                        registration.access.endpoint.url(),
                        entity_id,
                        &query.to_query_string(),
                    ))]
                });
                expand_registration_targets(registration, reads, &mut targets);
            }
            if targets.is_empty() {
                return Ok(ForwardReadOutcome::empty());
            }
            let matched = targets.len();
            let shape = shape_of(client_query);
            let answers = self
                .fan_out_read(
                    targets,
                    |registration, outcome| received_answer(registration, outcome, document(shape)).answer,
                    ForwardedGetConditions {
                        context,
                        triggering,
                        cacheability: Cacheability::Cacheable,
                        entity_map,
                    },
                )
                .await;
            let mut outcome = self.answered_states(read_outcome(answers, matched)).await;
            for answer in &mut outcome.results {
                settle(shape, entity_id, answer);
            }
            Ok(outcome)
        }
        .await;
        timer.finish_result(&result);
        result
    }
}

/// What the binding a query arrived on asks of the registrations it reads.
const fn read_intent(delivery: QueryDelivery) -> ReadIntent {
    match delivery {
        QueryDelivery::Query => ReadIntent::Query,
        QueryDelivery::Batch => ReadIntent::QueryBatch,
    }
}

/// The forwarded read one registration receives for a query, through the
/// binding it advertises (clauses 4.3.6.1, 6.23.1): a GET query, or a POST
/// batch query.
fn query_read(registration: &ContextSourceRegistration, query: ForwardedQuery, binding: ReadBinding) -> Vec<ForwardedRead> {
    let endpoint = registration.access.endpoint.url();
    match binding {
        ReadBinding::Query => get_reads(registration, query, build_query_url),
        ReadBinding::BatchQuery => {
            // Clause 6.23 carries the query in the body; the page size travels
            // as the URI parameter clause 6.3.10 defines for it.
            let page = format!("{}={}", ForwardedParameter::Limit, SOURCE_PAGE_SIZE);
            vec![match batch_query(&query) {
                Ok(body) => ForwardedRead::PostQuery {
                    url: build_batch_query_url(endpoint, &page),
                    body: Box::new(body),
                },
                // A query that cannot be written is the registration's failure
                // to prepare, which a read reports as its own.
                Err(error) => ForwardedRead::Get(Err(error)),
            }]
        }
    }
}
