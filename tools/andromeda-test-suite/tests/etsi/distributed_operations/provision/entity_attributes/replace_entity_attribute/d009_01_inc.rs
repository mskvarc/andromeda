//! ETSI `D009_01_inc`: a replacement of the `speed` of an entity an inclusive registration matches is forwarded to
//! the context source, and the broker answers a `speed` of 99 (clauses 4.3.3, 4.3.6.2, 5.6.19 and 6.3.18).

use crate::{
    distributed_operations::{
        forwarding_source::{register, serve, source_endpoint, stub, unregister, vehicle_retrieval_rule},
        provision::entity_attributes::attribute_sources::{replace_speed, speed_99_reply, vehicle_attribute_registration, vehicle_rule},
    },
    suite_keywords::{create_entity_with_context_string, delete_entities, retrieve_entity_json},
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Property},
};
use serde_json::json;

/// Pins: Check that one can replace an existing entity attribute, the changes will be forwarded to the Context Source
/// thanks to an inclusive registration
/// Clause: 4.3.3, 4.3.6.2, 5.6.19, 6.3.18
/// Corrected: `fix(D009_01_inc)`: attribute replace body and a parseable source stub (5.6.19)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: the case's teardown never stops the context source; the test's source goes when the test ends, which no
/// request of the case can observe.
/// Changed: the retrieval is held to a success status, where the case reads the body of any status.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d009_01_inc_replace_entity_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    let id = namespace.urn("Vehicle", 1)?;
    let vehicle = Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .property("speed", Property::builder(56).property("source", "Speedometer"))
        .build()?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle).await?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(&broker, &vocabulary, &vehicle_attribute_registration(&namespace, 1, &endpoint, None, &[])?).await?;
    let source = serve(source).await?;
    stub(&source, vehicle_rule(&namespace, Method::PUT, "", "/attrs/speed"), StatusCode::NO_CONTENT, None).await?;
    let linked = broker.with_context(vocabulary.context())?;
    replace_speed(&linked, &id).await?;
    stub(
        &source,
        vehicle_retrieval_rule(&namespace, "", 1),
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
