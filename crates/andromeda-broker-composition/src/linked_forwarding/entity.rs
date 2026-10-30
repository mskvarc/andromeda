//! The Core API half of [`LinkedForwarding`]: a Linked Entity is fetched as
//! the clause 5.7.1 retrieval of the entity, continuing the traversal from
//! it, and every source's entities are kept side by side.

use crate::linked_forwarding::{LinkedForwarding, linked_request, resolution, traversal_parameters::traversal_continuation};
use andromeda_entity::{error::EntityError, query::remote_linked::RemoteLinkedEntitySource};
use andromeda_federation::forwarded_query::client_query::ClientQuery;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_linked_retrieval::source::RemoteResolution;
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_request::entity::GetEntityParams;
use async_trait::async_trait;
use std::num::NonZeroU32;

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
impl RemoteLinkedEntitySource for LinkedForwarding {
    async fn fetch_linked(
        &self,
        id: &Identifier,
        remaining_depth: NonZeroU32,
        visited: &[Identifier],
        context: &ContextSpec,
    ) -> Result<RemoteResolution<ReceivedState>, EntityError> {
        let parameters = linked_retrieval_parameters(remaining_depth, visited);
        let client_query = ClientQuery::EntityRetrieve { id, params: &parameters };
        let request = linked_request(id, client_query, context);
        let forwarded = match self.federation.match_retrieve(&self.live_target, &request).await {
            Ok(matched) => self.federation.forward_retrieve(&self.live_target, request, matched.into_registrations()).await,
            Err(error) => Err(error),
        };
        Ok(resolution(id, forwarded, |gathered, _, entity| gathered.push(entity)))
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::linked_forwarding::entity::linked_retrieval_parameters;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_representation::join_mode::JoinMode;
    use std::num::NonZeroU32;

    fn uri(value: &str) -> Identifier {
        value.parse().expect("valid URI")
    }

    fn depth(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("positive depth")
    }

    /// The retrieval carries the traversal continuation and no other
    /// parameter: a Linked Entity is asked for whole, whatever narrowed the
    /// roots.
    #[test]
    fn the_forwarded_retrieval_carries_the_continuation_alone() {
        let visited = [uri("urn:ngsi-ld:Building:1"), uri("urn:ngsi-ld:Building:2")];

        let parameters = linked_retrieval_parameters(depth(3), &visited);

        pretty_assertions::assert_eq!(parameters.join, Some(JoinMode::Flat));
        pretty_assertions::assert_eq!(parameters.join_level.map(NonZeroU32::get), Some(2));
        pretty_assertions::assert_eq!(parameters.contained_by.as_deref(), Some(visited.as_slice()));
        assert!(parameters.attrs.is_none() && parameters.local.is_none());
    }
}
