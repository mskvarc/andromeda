//! The distributed part of a single-entity retrieval: the two clause 5.7.1.4
//! forwarding phases and what they bring back.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity_map::contribution::SourceContribution;
use andromeda_federation::{
    FederationService,
    entity_result::{collect_warnings, contributions_of, merge_retrieve_results},
    forwarded_query::client_query::ClientQuery,
    outcome::{ForwardReadOutcome, ForwardedEntities},
    request::{CsrModeFilter, EntityMapInUse, ForwardRetrieveRequest, TriggeringRequest},
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_http_response::{declined_representation::single_source_declined, problem_response::ProblemResponse};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_request::entity::GetEntityParams;
use axum::http::Uri as HttpUri;
use tracing::warn;

/// What the distributed part of a retrieval brought back (clause 5.7.1.4).
pub(crate) struct ForwardedRetrieval {
    /// The entity, merged from every source that held it.
    pub(crate) entity: Option<Entity>,
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
    pub(crate) entity_id: &'a Uri,
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
    /// representation (Table 6.4.3.2-3), which is the operation's answer.
    pub(crate) async fn run(&self, local_entity: Option<Entity>) -> Result<ForwardedRetrieval, ProblemResponse> {
        let primary = self.phase(CsrModeFilter::ExcludeAuxiliary).await;
        if let Some(status) = primary.sources.single_source_status {
            return Err(single_source_declined(status, self.uri));
        }
        let mut warnings = collect_warnings(&primary.results);
        let mut contributions: Vec<SourceContribution> = contributions_of(&primary.results).collect();
        let merged = merge_retrieve_results(local_entity, primary.results);

        // Phase 2: auxiliary sources only if an entity was found, since they
        // only supplement existing data (clause 4.3.6.2).
        let entity = match merged {
            Some(merged) => {
                let auxiliary = self.phase(CsrModeFilter::AuxiliaryOnly).await;
                warnings.extend(collect_warnings(&auxiliary.results));
                contributions.extend(contributions_of(&auxiliary.results));
                merge_retrieve_results(Some(merged), auxiliary.results)
            }
            None => None,
        };
        Ok(ForwardedRetrieval {
            entity,
            warnings,
            contributions,
        })
    }

    /// One forwarding phase over the registrations `mode_filter` admits.
    async fn phase(&self, mode_filter: CsrModeFilter) -> ForwardReadOutcome<ForwardedEntities> {
        self.forwarding
            .forward_retrieve(
                self.target,
                ForwardRetrieveRequest {
                    entity_id: self.entity_id,
                    exclude_registration: None,
                    client_query: ClientQuery::EntityRetrieve {
                        id: self.entity_id,
                        params: self.params,
                    },
                    context: self.context,
                    triggering: self.triggering,
                    mode_filter,
                    entity_map: self.entity_map,
                },
            )
            .await
            .unwrap_or_else(|error| {
                // Retrieve forwarding is best-effort: a source that cannot be
                // reached is a warning, not a failure.
                warn!(%error, "context source forwarding failed during the entity retrieval");
                ForwardReadOutcome::empty()
            })
    }
}
