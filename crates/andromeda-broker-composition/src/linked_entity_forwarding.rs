//! Resolving a Linked Entity through the registered context sources.
//!
//! Clause 4.5.23.1 lets a Relationship annotate its object with an
//! `objectType`, which declares that object an Internal Linked Entity: an
//! entity of the declared type that exists in the distributed system, whether
//! or not this broker stores it. The traversal in `andromeda-entity` answers
//! for what is stored locally and asks this source for the rest; this is where
//! "the rest" becomes a forwarded retrieval against the matching Context Source
//! Registrations.

use crate::{linked_resolution_failure_mapping::resolution_failure, linked_traversal_parameters::traversal_continuation};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    error::EntityError,
    query::remote_linked::{RemoteLinkedEntities, RemoteLinkedEntitySource},
};
use andromeda_federation::{
    FederationService,
    forwarded_query::client_query::ClientQuery,
    request::{CsrModeFilter, EntityMapInUse, ForwardRetrieveRequest, TriggeringRequest},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_representation::linked_resolution_failure::{LinkedResolutionFailure, LinkedResolutionFailureKind};
use andromeda_ngsi_ld_request::entity::GetEntityParams;
use async_trait::async_trait;
use std::{num::NonZeroU32, sync::Arc};
use tracing::warn;

/// Fetches an `objectType`-annotated Linked Entity from the context sources
/// registered for its identifier (clause 4.5.23.1).
#[derive(derive_more::Debug)]
pub(crate) struct ForwardedLinkedEntitySource {
    /// The forwarding service that owns registration matching and transport.
    #[debug(skip)]
    federation: Arc<FederationService>,
    /// Tenant live branch used by linked forwarding.
    live_target: ContextTarget,
}

impl ForwardedLinkedEntitySource {
    /// Resolve Linked Entities through the given forwarding service.
    pub(crate) const fn new(federation: Arc<FederationService>, live_target: ContextTarget) -> ForwardedLinkedEntitySource {
        ForwardedLinkedEntitySource { federation, live_target }
    }
}

/// The parameters of the forwarded retrieval, which are the traversal
/// continuation and nothing else.
fn linked_retrieval_parameters(depth: NonZeroU32, visited: &[Identifier]) -> GetEntityParams {
    let (join, join_level, contained_by) = traversal_continuation(depth, visited);
    GetEntityParams {
        join,
        join_level,
        contained_by,
        ..GetEntityParams::default()
    }
}

#[async_trait]
impl RemoteLinkedEntitySource for ForwardedLinkedEntitySource {
    async fn fetch_linked(
        &self,
        id: &Identifier,
        remaining_depth: NonZeroU32,
        visited: &[Identifier],
        context: &ContextSpec,
    ) -> Result<RemoteLinkedEntities, EntityError> {
        let parameters = linked_retrieval_parameters(remaining_depth, visited);
        let forwarded = self
            .federation
            .forward_retrieve(
                &self.live_target,
                ForwardRetrieveRequest {
                    entity_id: id,
                    exclude_registration: None,
                    client_query: ClientQuery::EntityRetrieve { id, params: &parameters },
                    context,
                    // A linked retrieval starts at this broker rather than being a
                    // hop of an inbound one, so there is no `Via` chain to extend
                    // and no headers to pass through.
                    triggering: TriggeringRequest::default(),
                    // Clause 4.3.6.2: an auxiliary context source only supplements
                    // an entity that already exists, never supplies a missing one.
                    mode_filter: CsrModeFilter::ExcludeAuxiliary,
                    entity_map: EntityMapInUse::None,
                },
            )
            .await;

        // A Linked Entity that cannot be reached degrades the join instead of
        // the request (clause 4.5.23): the Relationship keeps its plain URI and
        // the caller is told what is missing.
        match forwarded {
            Ok(outcome) => {
                let mut resolved = RemoteLinkedEntities::default();
                for source in outcome.results {
                    for warning in source.warnings {
                        warn!("Linked entity {} could not be fully resolved from a context source: {}", id, warning);
                        resolved.failures.push(resolution_failure(id, warning));
                    }
                    resolved.entities.extend(source.entities);
                }
                Ok(resolved)
            }
            Err(error) => {
                warn!("Failed to forward the linked retrieval of {}: {}", id, error);
                Ok(RemoteLinkedEntities {
                    entities: Vec::new(),
                    failures: vec![LinkedResolutionFailure {
                        id: id.clone(), // clone: the failure outlives the borrowed candidate id
                        kind: LinkedResolutionFailureKind::Unreachable,
                        detail: error.to_string(),
                    }],
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::linked_entity_forwarding::linked_retrieval_parameters;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_representation::join_mode::JoinMode;
    use std::num::NonZeroU32;

    fn uri(value: &str) -> Identifier {
        value.parse().expect("valid URI")
    }

    fn depth(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("positive depth")
    }

    /// Three levels left means the fetched entity plus two more hops, so the
    /// remote broker is asked for `joinLevel=2`, and the already-walked ids
    /// travel as `containedBy` (the clause 4.5.23 containedBy query parameter).
    #[test]
    fn the_forwarded_retrieval_joins_one_level_less_than_the_depth_that_remains() {
        let visited = [uri("urn:ngsi-ld:Building:1"), uri("urn:ngsi-ld:Building:2")];

        let parameters = linked_retrieval_parameters(depth(3), &visited);

        pretty_assertions::assert_eq!(parameters.join, Some(JoinMode::Flat));
        pretty_assertions::assert_eq!(parameters.join_level.map(NonZeroU32::get), Some(2));
        pretty_assertions::assert_eq!(parameters.contained_by.as_deref(), Some(visited.as_slice()));
        assert!(parameters.attrs.is_none() && parameters.local.is_none());
    }

    /// The last level asks for the entity alone: the clause 4.5.23 joinLevel
    /// query parameter has no zero-hop join, so the forwarded retrieval
    /// carries no `join` parameter.
    #[test]
    fn the_last_level_forwards_a_plain_retrieval() {
        let parameters = linked_retrieval_parameters(depth(1), &[uri("urn:ngsi-ld:Building:1")]);

        assert!(parameters.join.is_none());
        assert!(parameters.join_level.is_none());
        pretty_assertions::assert_eq!(parameters.contained_by.as_deref().map(<[Identifier]>::len), Some(1));
    }

    /// A traversal that has visited nothing yet sends no `containedBy` at all,
    /// rather than an empty one.
    #[test]
    fn an_empty_visited_list_is_omitted() {
        let parameters = linked_retrieval_parameters(depth(2), &[]);

        pretty_assertions::assert_eq!(parameters.join, Some(JoinMode::Flat));
        pretty_assertions::assert_eq!(parameters.join_level.map(NonZeroU32::get), Some(1));
        assert!(parameters.contained_by.is_none());
    }
}
