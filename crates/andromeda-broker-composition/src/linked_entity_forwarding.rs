//! Resolving a Linked Entity through the registered context sources.
//!
//! Clause 4.5.23.1 lets a Relationship annotate its object with an
//! `objectType`, which declares that object an Internal Linked Entity: an
//! entity of the declared type that exists in the distributed system, whether
//! or not this broker stores it. The traversal in `andromeda-entity` answers
//! for what is stored locally and asks this source for the rest; this is where
//! "the rest" becomes a forwarded retrieval against the matching Context Source
//! Registrations.

use crate::{linked_resolution_failure_mapping::resolution_failure, linked_traversal_parameters::append_traversal};
use andromeda_entity::{
    error::EntityError,
    query::remote_linked::{RemoteLinkedEntities, RemoteLinkedEntitySource},
};
use andromeda_federation::{
    FederationService,
    operation::ForwardingOperation,
    request::{CsrModeFilter, ForwardRetrieveRequest, TriggeringRequest},
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_representation::linked_resolution_failure::{LinkedResolutionFailure, LinkedResolutionFailureKind};
use async_trait::async_trait;
use std::{num::NonZeroU32, sync::Arc};
use tracing::warn;
use url::form_urlencoded;

/// Fetches an `objectType`-annotated Linked Entity from the context sources
/// registered for its identifier (clause 4.5.23.1).
#[derive(derive_more::Debug)]
pub(crate) struct ForwardedLinkedEntitySource {
    /// The forwarding service that owns registration matching and transport.
    #[debug(skip)]
    federation: Arc<FederationService>,
}

impl ForwardedLinkedEntitySource {
    /// Resolve Linked Entities through the given forwarding service.
    pub(crate) const fn new(federation: Arc<FederationService>) -> ForwardedLinkedEntitySource {
        ForwardedLinkedEntitySource { federation }
    }
}

/// Build the query string of the forwarded retrieval, which is the traversal
/// continuation and nothing else.
fn linked_retrieval_query(depth: NonZeroU32, visited: &[Uri]) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    append_traversal(&mut serializer, depth, visited);
    serializer.finish()
}

#[async_trait]
impl RemoteLinkedEntitySource for ForwardedLinkedEntitySource {
    async fn fetch_linked(&self, id: &Uri, remaining_depth: NonZeroU32, visited: &[Uri], context: &ContextSpec) -> Result<RemoteLinkedEntities, EntityError> {
        let query_string = linked_retrieval_query(remaining_depth, visited);
        let forwarded = self
            .federation
            .forward_retrieve(ForwardRetrieveRequest {
                entity_id: id,
                exclude_registration: None,
                query_string: &query_string,
                context,
                operation: ForwardingOperation::RetrieveEntity,
                // A linked retrieval starts at this broker rather than being a
                // hop of an inbound one, so there is no `Via` chain to extend
                // and no headers to pass through.
                triggering: TriggeringRequest::default(),
                // Clause 4.3.6.2: an auxiliary context source only supplements
                // an entity that already exists, never supplies a missing one.
                mode_filter: CsrModeFilter::ExcludeAuxiliary,
                entity_map: None,
            })
            .await;

        // A Linked Entity that cannot be reached degrades the join instead of
        // the request (clause 4.5.23): the Relationship keeps its plain URI and
        // the caller is told what is missing.
        match forwarded {
            Ok(sources) => {
                let mut resolved = RemoteLinkedEntities::default();
                for source in sources {
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
    use crate::linked_entity_forwarding::linked_retrieval_query;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use std::num::NonZeroU32;
    use url::form_urlencoded;

    fn uri(value: &str) -> Uri {
        value.parse().expect("valid URI")
    }

    fn depth(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("positive depth")
    }

    fn decode(query: &str) -> Vec<(String, String)> {
        form_urlencoded::parse(query.as_bytes())
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect()
    }

    /// Three levels left means the fetched entity plus two more hops, so the
    /// remote broker is asked for `joinLevel=2`, and the already-walked ids
    /// travel as `containedBy` (the clause 4.5.23 containedBy query parameter).
    #[test]
    fn the_forwarded_retrieval_joins_one_level_less_than_the_depth_that_remains() {
        let visited = [uri("urn:ngsi-ld:Building:1"), uri("urn:ngsi-ld:Building:2")];

        let query = linked_retrieval_query(depth(3), &visited);

        pretty_assertions::assert_eq!(
            decode(&query),
            vec![
                ("join".to_owned(), "flat".to_owned()),
                ("joinLevel".to_owned(), "2".to_owned()),
                ("containedBy".to_owned(), "urn:ngsi-ld:Building:1,urn:ngsi-ld:Building:2".to_owned()),
            ]
        );
    }

    /// Every reserved character the identifiers carry is percent-encoded by the
    /// serializer, so the comma-joined `containedBy` list survives the round
    /// trip as one parameter rather than several.
    #[test]
    fn the_visited_identifiers_are_percent_encoded_into_one_parameter() {
        let visited = [uri("urn:ngsi-ld:Building:1"), uri("urn:ngsi-ld:Building:2")];

        let query = linked_retrieval_query(depth(2), &visited);

        assert!(
            query.contains("containedBy=urn%3Angsi-ld%3ABuilding%3A1%2Curn%3Angsi-ld%3ABuilding%3A2"),
            "unexpected encoding: {query}"
        );
    }

    /// The last level asks for the entity alone: the clause 4.5.23 joinLevel
    /// query parameter has no zero-hop
    /// join, so the forwarded retrieval carries no `join` parameter.
    #[test]
    fn the_last_level_forwards_a_plain_retrieval() {
        let query = linked_retrieval_query(depth(1), &[uri("urn:ngsi-ld:Building:1")]);

        pretty_assertions::assert_eq!(decode(&query), vec![("containedBy".to_owned(), "urn:ngsi-ld:Building:1".to_owned())]);
    }

    /// A traversal that has visited nothing yet sends no `containedBy` at all,
    /// rather than an empty one.
    #[test]
    fn an_empty_visited_list_is_omitted() {
        let query = linked_retrieval_query(depth(2), &[]);

        pretty_assertions::assert_eq!(
            decode(&query),
            vec![("join".to_owned(), "flat".to_owned()), ("joinLevel".to_owned(), "1".to_owned())]
        );
    }
}
