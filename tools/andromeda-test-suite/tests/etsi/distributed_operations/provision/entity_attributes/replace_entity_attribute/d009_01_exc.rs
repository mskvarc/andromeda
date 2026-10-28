//! ETSI `D009_01_exc`: a replacement of the `speed` an exclusive registration holds is forwarded to the context source
//! once, and the broker answers the source's `speed` (clauses 4.3.3, 4.3.6.3, 5.6.19 and 6.3.18).

use crate::{
    distributed_operations::{
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, stub, stub_count, unregister, vehicle_retrieval_rule},
        provision::entity_attributes::attribute_sources::{replace_speed, speed_99_reply, vehicle_attribute_registration, vehicle_rule},
    },
    suite_keywords::{delete_entities, retrieve_entity_json},
    suite_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, registration::RegistrationMode};
use serde_json::json;

/// Pins: Check that one can replace an existing entity attribute, the changes will be forwarded to the Context Source
/// thanks to an exclusive registration
/// Clause: 4.3.3, 4.3.6.3, 5.6.19, 6.3.18
/// Corrected: `fix(D009_01_exc)`: valid exclusive setup, attribute replace body, and source stub (5.6.19)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: the case's teardown never stops the context source; the test's source goes when the test ends, which no
/// request of the case can observe.
/// Changed: the retrieval is held to a success status, where the case reads the body of any status.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d009_01_exc_replace_entity_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_locally(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let endpoint = source_endpoint(&source, &namespace, "/broker1")?;
    let _registration = register(
        &broker,
        &vocabulary,
        &vehicle_attribute_registration(&namespace, 1, &endpoint, Some(RegistrationMode::Exclusive), &["speed"])?,
    )
    .await?;
    let source = serve(source).await?;
    let rule = || vehicle_rule(&namespace, Method::PUT, "/broker1", "/attrs/speed");
    stub(&source, rule(), StatusCode::NO_CONTENT, None).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let linked = broker.with_context(vocabulary.context())?;
    replace_speed(&linked, &id).await?;
    expect_that!(stub_count(&source, &rule()).await?, eq(1));
    stub(
        &source,
        vehicle_retrieval_rule(&namespace, "/broker1", 1),
        StatusCode::OK,
        Some(&speed_99_reply(&namespace)),
    )
    .await?;
    let body = retrieve_entity_json(&linked.with_accept(Accept::any()), &id).await?;
    expect_that!(body.pointer("/speed/value"), some(eq(&json!(99))));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
