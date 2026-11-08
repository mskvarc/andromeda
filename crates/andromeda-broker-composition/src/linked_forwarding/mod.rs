//! Resolving a Linked Entity this broker does not hold through the registered
//! context sources (clause 4.5.23.1).
//!
//! Clause 4.5.23.1 lets a Relationship annotate its object with an
//! `objectType`, which declares that object an Internal Linked Entity: an
//! entity of the declared type that exists in the distributed system, whether
//! or not this broker holds it. The Core traversal and the temporal traversal
//! answer for what this broker holds and ask a remote source for the rest;
//! [`LinkedForwarding`] is that source for both. Fetching one Linked Entity is
//! the same forwarded retrieval whichever traversal asks: the same
//! registrations are reached, a source warning means the same unresolved
//! entity, and an unreachable forwarding degrades the join the same way. What
//! the two differ in is the retrieval's parameters and how the answers of
//! several sources are gathered, which each port implementation states.

mod entity;
mod resolution_failure;
mod temporal;
mod traversal_parameters;

use crate::linked_forwarding::resolution_failure::source_failures;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_federation::{
    FederationService,
    error::FederationError,
    forwarded_query::client_query::ClientQuery,
    outcome::{ForwardReadOutcome, ForwardedEntities, ForwardedTemporalEntities},
    request::{CsrModeFilter, EntityMapInUse, ForwardRetrieveRequest, TriggeringRequest},
    source_reach::SourceReach,
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_linked_retrieval::source::RemoteResolution;
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::temporal_history::TemporalHistory;
use std::sync::Arc;
use tracing::warn;

/// Fetches `objectType`-annotated Linked Entities, and their temporal
/// evolution, from the context sources registered for them (clause
/// 4.5.23.1).
#[derive(derive_more::Debug)]
pub(crate) struct LinkedForwarding {
    /// The forwarding service that owns registration matching and transport.
    #[debug(skip)]
    federation: Arc<FederationService>,
    /// The tenant's live branch, which every linked retrieval reads.
    live_target: ContextTarget,
}

impl LinkedForwarding {
    /// Resolve Linked Entities through the given forwarding service.
    pub(crate) const fn new(federation: Arc<FederationService>, live_target: ContextTarget) -> LinkedForwarding {
        LinkedForwarding { federation, live_target }
    }
}

/// The forwarded retrieval of the Linked Entity `id`, asking each source what
/// `client_query` states under `context`.
fn linked_request<'a>(id: &'a Identifier, client_query: ClientQuery<'a>, context: &'a ContextSpec) -> ForwardRetrieveRequest<'a> {
    ForwardRetrieveRequest {
        entity_id: id,
        reach: SourceReach::Every,
        client_query,
        context,
        // A linked retrieval starts at this broker rather than being a hop of
        // an inbound one, so there is no `Via` chain to extend and no headers
        // to pass through.
        triggering: TriggeringRequest::default(),
        // Clause 4.3.6.2: an auxiliary context source only supplements an
        // entity that already exists, never supplies a missing one.
        mode_filter: CsrModeFilter::ExcludeAuxiliary,
        entity_map: EntityMapInUse::None,
    }
}

/// One context source's answer to a forwarded linked retrieval.
trait LinkedAnswer {
    /// The form the source's entities arrive in.
    type Entity;

    /// The registration the source answered for, the entities it answered
    /// with, and the clause 6.3.17 warnings the attempt raised.
    fn into_parts(self) -> (Option<Identifier>, Vec<Self::Entity>, Vec<NgsiLdWarning>);
}

impl LinkedAnswer for ForwardedEntities {
    type Entity = ReceivedState;

    fn into_parts(self) -> (Option<Identifier>, Vec<ReceivedState>, Vec<NgsiLdWarning>) {
        (self.registration_id, self.entities, self.warnings)
    }
}

impl LinkedAnswer for ForwardedTemporalEntities {
    type Entity = TemporalHistory;

    fn into_parts(self) -> (Option<Identifier>, Vec<TemporalHistory>, Vec<NgsiLdWarning>) {
        (self.registration_id, self.entities, self.warnings)
    }
}

/// What the forwarded retrieval of the Linked Entity `id` resolved, each
/// source's entities taken into the resolution by `gather` beside the
/// registration of the source that answered them.
///
/// A Linked Entity that cannot be reached degrades the join instead of the
/// request (clause 4.5.23): the Relationship keeps its plain URI and the
/// caller is told what is missing, so a failed forwarding is an answer too.
fn resolution<A: LinkedAnswer, T>(
    id: &Identifier,
    forwarded: Result<ForwardReadOutcome<A>, FederationError>,
    mut gather: impl FnMut(&mut Vec<T>, Option<&Identifier>, A::Entity),
) -> RemoteResolution<T> {
    match forwarded {
        Ok(outcome) => {
            let mut resolved = RemoteResolution {
                entities: Vec::new(),
                failures: Vec::new(),
            };
            for source in outcome.results {
                let (registration, entities, warnings) = source.into_parts();
                resolved.failures.extend(source_failures(id, warnings));
                for entity in entities {
                    gather(&mut resolved.entities, registration.as_ref(), entity);
                }
            }
            resolved
        }
        Err(error) => {
            warn!("Failed to forward the linked retrieval of {}: {}", id, error);
            RemoteResolution::unreachable(id, &error)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::linked_forwarding::resolution;
    use andromeda_federation::{
        error::FederationError,
        outcome::{ForwardReadOutcome, ForwardedTemporalEntities, MatchedSources},
    };
    use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{registration::forwarding_mode::ForwardingMode, temporal::temporal_history::TemporalHistory};
    use andromeda_ngsi_ld_representation::linked_resolution_failure::{LinkedResolutionFailure, LinkedResolutionFailureKind};

    fn uri(value: &str) -> Identifier {
        value.parse().expect("valid URI")
    }

    fn answer(warnings: Vec<NgsiLdWarning>) -> ForwardedTemporalEntities {
        ForwardedTemporalEntities {
            registration_id: Some(uri("urn:ngsi-ld:ContextSourceRegistration:1")),
            mode: ForwardingMode::Inclusive,
            entities: Vec::new(),
            warnings,
            linked_map: None,
            partial_ranges: Vec::new(),
        }
    }

    /// Every source's warnings become failures naming the Linked Entity, in
    /// source order.
    #[test]
    fn each_source_warning_is_a_failure_of_the_linked_entity() {
        let id = uri("urn:ngsi-ld:Building:1");
        let outcome = ForwardReadOutcome {
            results: vec![
                answer(vec![NgsiLdWarning::new(WarnCode::MiscellaneousWarning, "timeout".to_owned())]),
                answer(vec![NgsiLdWarning::new(WarnCode::RevalidationFailed, "not NGSI-LD".to_owned())]),
            ],
            sources: MatchedSources::default(),
        };

        let resolved = resolution(&id, Ok(outcome), |gathered: &mut Vec<_>, _, history| gathered.push(history));

        let kinds: Vec<_> = resolved.failures.iter().map(|failure| (failure.id.clone(), failure.kind)).collect();
        pretty_assertions::assert_eq!(
            kinds,
            vec![
                (id.clone(), LinkedResolutionFailureKind::Unreachable),
                (id, LinkedResolutionFailureKind::UnusableResponse),
            ]
        );
    }

    /// A forwarding that fails outright leaves the Linked Entity unresolved
    /// with the forwarding error as the detail, and fails nothing else.
    #[test]
    fn a_failed_forwarding_is_one_unreachable_failure() {
        let id = uri("urn:ngsi-ld:Building:1");
        let error = FederationError::LoopDetected {
            registration: uri("urn:ngsi-ld:ContextSourceRegistration:1"),
        };
        let detail = error.to_string();

        let resolved = resolution::<ForwardedTemporalEntities, TemporalHistory>(&id, Err(error), |gathered, _, history| gathered.push(history));

        assert!(resolved.entities.is_empty());
        pretty_assertions::assert_eq!(
            resolved.failures,
            vec![LinkedResolutionFailure {
                id,
                kind: LinkedResolutionFailureKind::Unreachable,
                detail,
            }]
        );
    }
}
