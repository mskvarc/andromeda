//! A retrieval of an entity whose `speed` an exclusive context source holds asks the source and answers the `speed`
//! merged into the broker's entity (clauses 4.3.3, 4.3.6.3 and 5.7.1).

use crate::distributed_operations::{
    consumption::entity::retrieve_entity::vehicle_sources::{exclusive_speed_source, torn_down},
    forwarding_source::{stub_count, vehicle_retrieval_rule},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::headers::Accept;

/// Pins: Check that if one retrieves entity living on a Context Source from a Context Broker, and the registration
/// only allows the retrieveEntity operation, the request is forwarded as expected
/// Clause: 4.3.3, 4.3.6.3, 5.7.1
/// Expected: the retrieve by id reaches the source, and the answer holds the merged member (clause 5.7.1.4)
#[gtest]
#[tokio::test]
async fn d010_01_exc_query_context_broker_and_retrieve_entity_by_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let (vocabulary, source, _registrations) = exclusive_speed_source(&broker, &namespace).await?;
    let retrieved = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve(&namespace.urn("Vehicle", 1)?)
        .send()
        .await;
    verify_that!(&retrieved, answered(StatusCode::OK))?;
    let body = json_body(&retrieved?)?;
    expect_that!(body.pointer("/speed"), some(anything()));
    expect_that!(stub_count(&source, &vehicle_retrieval_rule(&namespace, "/broker1", 1)).await?, gt(0));
    torn_down(&broker, &namespace, &[1]).await
}
