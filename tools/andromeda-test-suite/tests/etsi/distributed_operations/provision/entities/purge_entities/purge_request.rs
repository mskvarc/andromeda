//! The Purge Entities the purge cases send, and the stub rule of the purge a source receives.

use crate::distributed_operations::{fork_rule::ForkRule, forwarding_source::source_path};
use andromeda_test_suite::{broker::BrokerClient, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::send_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::operation::PurgeEntities, query::SelectorType};

/// `Purge Entities` of the type `Vehicle` at `/entities/?type=Vehicle`, with the vocabulary in the `Link` header:
/// requires 204.
pub(crate) async fn purge_vehicles(broker: &BrokerClient, vocabulary: &Vocabulary) -> Result<()> {
    let vehicles = "Vehicle".parse::<SelectorType>()?;
    let operation = PurgeEntities::builder().entity_type(&vehicles).build();
    let answer = send_to(&broker.with_context(vocabulary.context())?, &operation, "entities/?type=Vehicle").await;
    verify_that!(&answer, answered(StatusCode::NO_CONTENT))
}

/// The stub rule of the purge of the type `Vehicle` at the source's `<below>`: the entities path with exactly the
/// query `type=Vehicle`.
pub(crate) fn purge_rule(namespace: &Namespace, below: &str) -> ForkRule {
    ForkRule::literal(Method::DELETE, &source_path(namespace, &format!("{below}/ngsi-ld/v1/entities")), "type=Vehicle")
}
