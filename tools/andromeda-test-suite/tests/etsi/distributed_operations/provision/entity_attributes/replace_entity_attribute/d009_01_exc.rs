//! A replacement of the `speed` an exclusive registration holds is forwarded to the context source once, and the broker
//! answers the source's `speed` (clauses 4.3.3, 4.3.6.3, 5.6.19 and 6.3.18).

use crate::{
    distributed_operations::{
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, stub, stub_count, unregister, vehicle_retrieval_rule},
        provision::entity_attributes::attribute_sources::{replace_speed, speed_99_reply, vehicle_attribute_registration, vehicle_rule},
    },
    entity_operations::{delete_entities, retrieve_entity_json},
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, registration::RegistrationMode};
use serde_json::json;

/// Pins: Check that one can replace an existing entity attribute, the changes will be forwarded to the Context Source
/// thanks to an exclusive registration
/// Clause: 4.3.3, 4.3.6.3, 5.6.19, 6.3.18
/// Expected: the exclusive setup is valid, the body is an attribute replacement, and the source answers (clause 5.6.19)
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
