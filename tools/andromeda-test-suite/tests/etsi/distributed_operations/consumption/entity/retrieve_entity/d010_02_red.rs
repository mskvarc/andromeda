//! A retrieval with `local=true` of an entity only redirect context sources hold is not forwarded and finds no entity
//! (clauses 4.3.3, 4.3.6.3, 5.7.1 and 6.3.18).

use crate::distributed_operations::{
    consumption::entity::retrieve_entity::vehicle_sources::{redirect_rules, redirect_vehicle_sources, torn_down},
    forwarding_source::stub_count,
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, query::Locality};

/// Pins: Check that when retrieving an entity locally, the Context Broker does not forward the request to the Context
/// Sources and raises an error.
/// Clause: 4.3.3, 4.3.6.3, 5.7.1
#[gtest]
#[tokio::test]
async fn d010_02_red_query_context_broker_and_retrieve_entity_by_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let (vocabulary, source, _registrations) = redirect_vehicle_sources(&broker, &namespace).await?;
    let retrieved = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve(&namespace.urn("Vehicle", 1)?)
        .locality(Locality::Local)
        .send()
        .await;
    expect_that!(&retrieved, answered(StatusCode::NOT_FOUND));
    let [first, second] = redirect_rules(&namespace);
    expect_that!(stub_count(&source, &first).await?, eq(0));
    expect_that!(stub_count(&source, &second).await?, eq(0));
    torn_down(&broker, &namespace, &[1, 2]).await
}
