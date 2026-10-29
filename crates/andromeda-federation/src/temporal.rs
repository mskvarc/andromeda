//! Federated temporal entity query and retrieve operations (clauses 5.7.3, 5.7.4, 6.3.17).

use crate::{
    FederationService,
    cache::Cacheability,
    entity_read::own_loop_warning,
    error::FederationError,
    forwarded_query::client_query::ClientQuery,
    forwarding_context::BodyPresence,
    forwarding_url::{build_temporal_query_url, build_temporal_retrieve_url},
    operation::ForwardingOperation,
    outcome::{ForwardReadOutcome, ForwardedTemporalEntities, MatchedSources},
    query_forwarding_error::QueryForwardingError,
    read_fan_out::{ForwardedGetConditions, ForwardedRead, expand_registration_targets, read_outcome},
    received_temporal_answer::ReceivedTemporalAnswer,
    registration_narrowing::{Narrowing, get_reads},
    registration_selection::{RegistrationSelection, RegistrationServing},
    request::{ForwardRetrieveRequest, ForwardTemporalQueryRequest, SourcePaging},
    split_entities::SplitEntities,
    term_resolver::TermResolver,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_model::{received::ContextualEntity, registration::forwarding_mode::ForwardingMode};
use andromeda_ngsi_ld_request::query::entity_selector::EntitySelectorParams;

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
            geo,
            mode_filter,
            only_registration,
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
                    exclude_registration: None,
                    csf: csf_expr,
                    scope_q: client_query.scope_q.as_ref(),
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
        for registration in registrations
            .into_iter()
            .filter(|registration| only_registration.is_none_or(|only| only == &registration.id))
        {
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
            SourcePaging::Page(_) => {
                let received = self
                    .fan_out_read(
                        targets,
                        |registration, outcome| FederationService::received_temporal_answer(registration, outcome).answer,
                        conditions,
                    )
                    .await;
                self.read_temporal_answers(received).await
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
            .select_registrations(
                context,
                RegistrationSelection {
                    selectors: &[EntitySelectorParams {
                        id: Some(vec![entity_id.clone()]), // clone: the selector owns its identifiers
                        ..EntitySelectorParams::default()
                    }],
                    exclude_registration,
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
            return Ok(ForwardReadOutcome::empty());
        }
        let matched = targets.len();
        let received = self
            .fan_out_read(
                targets,
                |registration, outcome| FederationService::received_temporal_answer(registration, outcome).answer,
                ForwardedGetConditions {
                    context,
                    triggering,
                    cacheability: Cacheability::Uncacheable,
                    entity_map,
                },
            )
            .await;
        let answers = self.read_temporal_answers(received).await;
        Ok(self
            .answered_temporal(read_outcome(answers, matched), context, client_query.timeproperty())
            .await)
    }
}
