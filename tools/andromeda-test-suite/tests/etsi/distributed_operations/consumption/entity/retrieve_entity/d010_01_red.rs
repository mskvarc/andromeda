//! ETSI `D010_01_red`: an entity two redirect context sources each hold a part of is retrieved through the broker
//! with the parts of both (clauses 4.3.3, 4.3.6.3 and 5.7.1).

use crate::distributed_operations::{
    consumption::entity::retrieve_entity::vehicle_sources::{redirect_rules, redirect_vehicle_sources, torn_down},
    forwarding_source::stub_count,
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::headers::Accept;

/// Pins: Check that if one retrieves an entity on the Context Source, entity gets redirected correctly
/// Clause: 4.3.3, 4.3.6.3, 5.7.1
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: the teardown deletes of the registrations are held to the 204 a deleted registration answers, not only to
/// a status below 400.
#[gtest]
#[tokio::test]
async fn d010_01_red_query_context_broker_and_retrieve_entity_by_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let (vocabulary, source, _registrations) = redirect_vehicle_sources(&broker, &namespace).await?;
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
    expect_that!(body.pointer("/isParked"), some(anything()));
    expect_that!(body.pointer("/isParked2"), some(anything()));
    let [first, second] = redirect_rules(&namespace);
    expect_that!(stub_count(&source, &first).await?, gt(0));
    expect_that!(stub_count(&source, &second).await?, gt(0));
    torn_down(&broker, &namespace, &[1, 2]).await
}
