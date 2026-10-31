//! A replacement of the `speed` of an entity an inclusive registration matches is forwarded to the context source, and
//! the broker answers a `speed` of 99 (clauses 4.3.3, 4.3.6.2, 5.6.19 and 6.3.18).

use crate::{
    distributed_operations::{
        forwarding_source::{register, serve, source_endpoint, stub, unregister, vehicle_retrieval_rule},
        provision::entity_attributes::attribute_sources::{replace_speed, speed_99_reply, vehicle_attribute_registration, vehicle_rule},
    },
    entity_operations::{create_entity_with_context_string, delete_entities, retrieve_entity_json},
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
/// Expected: the body is an attribute replacement and the source answers with a parseable body (clause 5.6.19)
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
