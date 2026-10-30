//! The distributed part of a single-entity retrieval: the two clause 5.7.1.4
//! forwarding phases and what they bring back.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity_map::contribution::SourceContribution;
use andromeda_federation::{
    FederationService,
    entity_result::{collect_warnings, contributions_of, merge_retrieve_results},
    error::FederationError,
    forwarded_query::client_query::ClientQuery,
    matched_registrations::PhasedRegistrations,
    outcome::{ForwardReadOutcome, ForwardedEntities},
    request::{CsrModeFilter, EntityMapInUse, ForwardRetrieveRequest, TriggeringRequest},
    source_reach::SourceReach,
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_http_response::{
    declined_representation::single_source_declined,
    forwarding_failure::local_federation_failure,
    problem_response::ProblemResponse,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_request::entity::GetEntityParams;
use axum::http::Uri as HttpUri;
use tracing::warn;

/// What the distributed part of a retrieval brought back (clause 5.7.1.4).
pub(crate) struct ForwardedRetrieval {
    /// The entity, merged on the model from every source that held it.
    pub(crate) entity: Option<ReceivedState>,
    /// The clause 6.3.17 warnings of both forwarding phases.
    pub(crate) warnings: Vec<NgsiLdWarning>,
    /// What each source contributed to the map of the request (clause 5.2.39).
    pub(crate) contributions: Vec<SourceContribution>,
}

/// The two clause 5.7.1.4 forwarding phases of one retrieval: the exclusive,
/// redirect and inclusive sources first, then the auxiliary ones (clause
/// 4.3.6.2).
pub(crate) struct RetrieveForwarding<'a> {
    /// The forwarding policy.
    pub(crate) forwarding: &'a FederationService,
    /// Live or snapshot branch selected for the request.
    pub(crate) target: &'a ContextTarget,
    /// The request URI problem responses name.
    pub(crate) uri: &'a HttpUri,
    /// The entity being retrieved.
    pub(crate) entity_id: &'a Identifier,
    /// The retrieval parameters as the client wrote them.
    pub(crate) params: &'a GetEntityParams,
    /// The request's JSON-LD context.
    pub(crate) context: &'a ContextSpec,
    /// The triggering request's `Via` chain and headers.
    pub(crate) triggering: TriggeringRequest<'a>,
    /// The `EntityMap` the retrieval runs under.
    pub(crate) entity_map: EntityMapInUse<'a>,
}

impl RetrieveForwarding<'_> {
    /// Run both phases, merged onto whatever the broker holds itself.
    ///
    /// A source that could not be reached is a clause 6.3.17 warning, never a
    /// failure of the retrieval.
    ///
    /// # Errors
    ///
    /// Returns the answer of the one proxied source that declined the
    /// representation (Table 6.4.3.2-3), which is the operation's answer, and
    /// the 503 of a registration store that is saturated or unreachable.
    pub(crate) async fn run(&self, local_entity: Option<ReceivedState>) -> Result<ForwardedRetrieval, ProblemResponse> {
        // Both phases match the same registrations, so they are read once.
        let phased = self.matched().await?;
        let primary = self.phase(phased.primary).await?;
        if let Some(status) = primary.sources.single_source_status {
            return Err(single_source_declined(status, self.uri));
        }
        let mut warnings = collect_warnings(&primary.results);
        let mut contributions: Vec<SourceContribution> = contributions_of(&primary.results).collect();
        let merged = merge_retrieve_results(local_entity, primary.results);

        // Clause 5.7.1.4 answers ResourceNotFound only when no matching
        // registration applies, and a matching auxiliary one does: its sources
        // are asked even when nobody else held the entity, and their
        // Attributes fill whatever no other holder supplied.
        let auxiliary = self.phase(phased.auxiliary).await?;
        warnings.extend(collect_warnings(&auxiliary.results));
        contributions.extend(contributions_of(&auxiliary.results));
        let entity = merge_retrieve_results(merged, auxiliary.results);
        Ok(ForwardedRetrieval {
            entity,
            warnings,
            contributions,
        })
    }

    /// The forwarded retrieval, as both phases send it.
    const fn request(&self) -> ForwardRetrieveRequest<'_> {
        ForwardRetrieveRequest {
            entity_id: self.entity_id,
            reach: SourceReach::Every,
            client_query: ClientQuery::EntityRetrieve {
                id: self.entity_id,
                params: self.params,
            },
            context: self.context,
            triggering: self.triggering,
            mode_filter: CsrModeFilter::All,
            entity_map: self.entity_map,
        }
    }

    /// Read the registrations the retrieval matches, split by phase.
    ///
    /// One that could not be read because this broker's own store is saturated
    /// or unreachable fails the retrieval rather than hide every source.
    async fn matched(&self) -> Result<PhasedRegistrations, ProblemResponse> {
        match self.forwarding.match_retrieve(self.target, &self.request()).await {
            Ok(matched) => Ok(matched.into_phases()),
            Err(error) => self.failed(&error).map(|()| PhasedRegistrations::default()),
        }
    }

    /// One forwarding phase over the registrations it contacts.
    ///
    /// A source that could not be reached leaves the phase empty.
    async fn phase(&self, registrations: Vec<ContextSourceRegistration>) -> Result<ForwardReadOutcome<ForwardedEntities>, ProblemResponse> {
        match self.forwarding.forward_retrieve(self.target, self.request(), registrations).await {
            Ok(outcome) => Ok(outcome),
            Err(error) => self.failed(&error).map(|()| ForwardReadOutcome::empty()),
        }
    }

    /// The failure of a forwarding step: the failure of this broker's own
    /// store is the retrieval's, while retrieve forwarding is best-effort for
    /// a source that cannot be reached, which is only logged.
    fn failed(&self, error: &FederationError) -> Result<(), ProblemResponse> {
        if let Some(report) = local_federation_failure(error) {
            // clone: the problem response owns the request URI it names
            return Err(report.with_instance(self.uri.clone()));
        }
        warn!(%error, "context source forwarding failed during the entity retrieval");
        Ok(())
    }
}
