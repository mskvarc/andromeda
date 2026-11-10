//! Federated temporal entity query and retrieve operations (clauses 5.7.3, 5.7.4, 6.3.17).

use crate::{
    FederationService,
    cache::Cacheability,
    entity_read::own_loop_warning,
    error::FederationError,
    forwarded_query::client_query::ClientQuery,
    forwarding_context::BodyPresence,
    forwarding_url::{build_temporal_query_url, build_temporal_retrieve_url},
    instance_budget::InstanceBudget,
    operation::ForwardingOperation,
    outcome::{ForwardReadOutcome, ForwardedTemporalEntities, MatchedSources},
    query_forwarding_error::QueryForwardingError,
    read_fan_out::{ForwardedGetConditions, ForwardedRead, SourceAnswer, expand_registration_targets, read_outcome},
    received_temporal_answer::ReceivedTemporalAnswer,
    registration_narrowing::{Narrowing, get_reads},
    registration_selection::{RegistrationSelection, RegistrationServing},
    request::{ForwardRetrieveRequest, ForwardTemporalQueryRequest, HistoryBudget, SourcePaging, TemporalAnswer},
    result_budget::{ResultBudget, ResultBudgetExceeded},
    source_walk::{ReadPage, WalkedAnswer},
    split_entities::SplitEntities,
    term_resolver::TermResolver,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_model::{
    received::ContextualEntity,
    registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode},
};
use andromeda_ngsi_ld_request::query::entity_selector::EntitySelectorParams;

/// The pages every source answered, each charged to `budget` as a walk charges
/// its pages: every element the source sent as a row, and the entities it
/// named as entities (clause 4.12).
///
/// # Errors
///
/// Returns [`ResultBudgetExceeded`] once everything charged to `budget`, by
/// these pages and every earlier one of the request, exceeds it.
fn charged_pages(
    received: Vec<SourceAnswer<(ReceivedTemporalAnswer<ContextualEntity>, usize)>>,
    budget: &ResultBudget,
) -> Result<Vec<SourceAnswer<ReceivedTemporalAnswer<ContextualEntity>>>, ResultBudgetExceeded> {
    received
        .into_iter()
        .map(|SourceAnswer { mode, error_status, entry }| {
            let entry = match entry {
                Some((answer, sent)) => {
                    budget.charge_page(sent, answer.entities.iter().map(ReceivedTemporalAnswer::identifier))?;
                    Some(answer)
                }
                None => None,
            };
            Ok(SourceAnswer { mode, error_status, entry })
        })
        .collect()
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
    /// Forward a temporal entity query to the matching registrations (clause
    /// 5.7.4.4), each sent the part of the client's query it is registered for,
    /// and read as much of each source's answer as `paging` asks for: every
    /// page it links with `next` (clauses 4.3.6.7 and 5.5.9.1), or one
    /// explicit page. Either is charged to the request's budget.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when the query's entity type
    /// cannot be expanded against the request's context,
    /// [`FederationError::Registration`] when the matching registrations
    /// cannot be read, and [`FederationError::Conflict`] when the selected
    /// registrations are mutually exclusive, each as
    /// [`QueryForwardingError::Federation`], and
    /// [`QueryForwardingError::TooManyResults`] when the answers, walked or
    /// paged, exceed the request's budget (clause 4.12). A source that fails
    /// to answer does not fail the call: its failure travels as a warning.
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
            geo,
            mode_filter,
            reach,
            entity_map,
            paging,
        } = request;
        if self.is_loop_detected(triggering.via) {
            return Ok(own_loop_outcome());
        }
        let client = ClientQuery::TemporalQuery(client_query);
        let registrations = self
            .select_registrations(
                context,
                RegistrationSelection {
                    selectors: &client.selectors(),
                    reach,
                    csf: csf_expr,
                    scope_q: split.registration_scope_query(client_query.scope_q.as_ref()),
                    geo,
                    serving: RegistrationServing::Operation(operation),
                    mode_filter,
                    entity_map,
                },
            )
            .await?;
        // Clause 4.3.6.3: each information block that covers part of the query
        // is a forwarded request of its own.
        let mut targets = Vec::with_capacity(registrations.len());
        let resolver = TermResolver::new(&self.engine, context);
        let narrowing = Narrowing::builder()
            .client(client)
            .inbound(context)
            .resolver(&resolver)
            .split(split)
            .source_page(paging.explicit_page())
            .temporal_answer(paging.temporal_answer())
            .presence(BodyPresence::Absent)
            .build();
        for registration in registrations {
            let reads = self
                .registration_queries(&registration, &narrowing)
                .await?
                .into_reads(|query| get_reads(&registration, query, build_temporal_query_url));
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
            SourcePaging::Walk(budget) => {
                let walked = self
                    .fan_out_walk::<ReceivedTemporalAnswer<ContextualEntity>>(targets, conditions, budget)
                    .await?;
                self.read_temporal_answers(walked).await
            }
            SourcePaging::WalkHistory(budget) => self.fan_out_history_walk(targets, conditions, budget).await?,
            SourcePaging::Page { budget, .. } => {
                let received = self
                    .fan_out_read(
                        targets,
                        |registration, outcome| {
                            let ReadPage { answer, sent } = FederationService::received_temporal_answer(registration, outcome);
                            answer.map(|answer| (answer, sent))
                        },
                        conditions,
                    )
                    .await;
                self.read_temporal_answers(charged_pages(received, budget)?).await
            }
        };
        Ok(self
            .answered_temporal(read_outcome(answers, matched), context, client_query.timeproperty_or_default())
            .await)
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
        let targets = match self.retrieve_reads(target, request, TemporalAnswer::AsRequested).await? {
            RetrieveReads::Settled(outcome) => return Ok(outcome),
            RetrieveReads::Targets(targets) => targets,
        };
        let matched = targets.len();
        let received = self
            .fan_out_read(
                targets,
                |registration, outcome| FederationService::received_temporal_answer(registration, outcome).answer,
                retrieve_conditions(request),
            )
            .await;
        let answers = self.read_temporal_answers(received).await;
        Ok(self
            .answered_temporal(read_outcome(answers, matched), request.context, request.client_query.timeproperty())
            .await)
    }

    /// Forward a temporal entity retrieval to the matching context sources
    /// (clause 5.7.3.4), each asked for the normalized instances of the window
    /// whatever representation the client asked for, and read to the end of
    /// the window past every clause 6.3.10 partial answer under `instances`.
    ///
    /// Clause 5.7.3.4 builds the aggregated representation from the instances
    /// merged into S1, which no source's own aggregate is once another holder
    /// holds part of the entity, so the instances are what is gathered.
    ///
    /// # Errors
    ///
    /// The failures of [`FederationService::forward_temporal_retrieve`], each
    /// as [`QueryForwardingError::Federation`], and
    /// [`QueryForwardingError::TooManyInstances`] once the sources together
    /// answer with more instances than `instances` allows (clause 4.12).
    pub async fn forward_temporal_retrieve_history(
        &self,
        target: &ContextTarget,
        request: ForwardRetrieveRequest<'_>,
        instances: &InstanceBudget,
    ) -> Result<ForwardReadOutcome<ForwardedTemporalEntities>, QueryForwardingError> {
        let targets = match self.retrieve_reads(target, request, TemporalAnswer::Instances).await? {
            RetrieveReads::Settled(outcome) => return Ok(outcome),
            RetrieveReads::Targets(targets) => targets,
        };
        let matched = targets.len();
        // One entity is retrieved, so the entity budget bounds nothing a source
        // answering it could exceed; the instances are what is bounded.
        let entities = ResultBudget::new(self.result_limits);
        let answers = self
            .fan_out_history_walk(
                targets,
                retrieve_conditions(request),
                HistoryBudget {
                    entities: &entities,
                    instances,
                },
            )
            .await?;
        Ok(self
            .answered_temporal(read_outcome(answers, matched), request.context, request.client_query.timeproperty())
            .await)
    }

    /// The reads a temporal retrieval sends each matching registration, each
    /// asking for `answer`; or the outcome settled without contacting any.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::Registration`] when the matching
    /// registrations cannot be read, and [`FederationError::Conflict`] when
    /// the selected registrations are mutually exclusive.
    async fn retrieve_reads(
        &self,
        target: &ContextTarget,
        request: ForwardRetrieveRequest<'_>,
        answer: TemporalAnswer,
    ) -> Result<RetrieveReads, FederationError> {
        if !target.permits_federation() {
            return Ok(RetrieveReads::Settled(ForwardReadOutcome::empty()));
        }
        let ForwardRetrieveRequest {
            entity_id,
            reach,
            client_query,
            context,
            triggering,
            mode_filter,
            entity_map,
        } = request;
        if self.is_loop_detected(triggering.via) {
            return Ok(RetrieveReads::Settled(own_loop_outcome()));
        }
        let registrations = self
            .select_registrations(
                context,
                RegistrationSelection {
                    selectors: &[EntitySelectorParams {
                        id: Some(vec![entity_id.clone()]),
                        ..EntitySelectorParams::default()
                    }],
                    reach,
                    csf: None,
                    scope_q: None,
                    geo: None,
                    serving: RegistrationServing::Operation(ForwardingOperation::RetrieveTemporalEntity),
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
        let narrowing = Narrowing::builder()
            .client(client_query)
            .inbound(context)
            .resolver(&resolver)
            .split(SplitEntities::Complete)
            .temporal_answer(answer)
            .presence(BodyPresence::Absent)
            .build();
        for registration in registrations {
            let reads = self.registration_queries(&registration, &narrowing).await?.into_reads(|query| {
                vec![ForwardedRead::Get(build_temporal_retrieve_url(
                    registration.access.endpoint.url(),
                    entity_id,
                    &query.to_query_string(),
                ))]
            });
            expand_registration_targets(registration, reads, &mut targets);
        }
        if targets.is_empty() {
            return Ok(RetrieveReads::Settled(ForwardReadOutcome::empty()));
        }
        Ok(RetrieveReads::Targets(targets))
    }
}

/// What a temporal retrieval's forwarding has to do once its registrations are
/// matched.
enum RetrieveReads {
    /// Nothing is contacted: the branch reaches no source, the request already
    /// looped back here, or no registration matched.
    Settled(ForwardReadOutcome<ForwardedTemporalEntities>),
    /// The read each matching registration is sent.
    Targets(Vec<(ContextSourceRegistration, ForwardedRead)>),
}

/// The conditions every forwarded copy of a temporal retrieval is sent under.
/// Clause 6.3.17 allows caching for `/entities` only.
const fn retrieve_conditions(request: ForwardRetrieveRequest<'_>) -> ForwardedGetConditions<'_> {
    ForwardedGetConditions {
        context: request.context,
        triggering: request.triggering,
        cacheability: Cacheability::Uncacheable,
        entity_map: request.entity_map,
    }
}
