//! A registration created with the `@context` of its `application/ld+json` body is retrieved compacted under that
//! context or expanded without one (clauses 5.9.2 and 6.3.5).

use crate::{
    entity_operations::ld_json_body_client,
    registration_payloads::{VEHICLE_PARKING_TERMS, vehicle_parking_registration},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    config::SuiteConfig,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, registration::RegistrationId};
use serde_json::{Value, json};

/// The setup of the cases: creates the Vehicle `urn:ngsi-ld:Vehicle:<ns>-A456` registration with the vocabulary as
/// the body's `@context` (201, an empty body), answering the vocabulary, the registration's identifier and the cleanup
/// that deletes it.
async fn created(broker: &BrokerClient, namespace: &Namespace) -> Result<(Vocabulary, RegistrationId, Cleanup)> {
    let vocabulary = Vocabulary::register(broker, namespace, &VEHICLE_PARKING_TERMS).await?;
    let registration = vehicle_parking_registration(namespace, SuiteConfig::get()?.unreachable_origin(), namespace.urn("Vehicle", "A456")?, None)?;
    let cleanup = Cleanup::of(broker).registration_of(&registration);
    let client = ld_json_body_client(broker, &vocabulary)?;
    let created = client.with_responses().registrations().register(&registration).send().await;
    verify_that!(&created, answered(StatusCode::CREATED))?;
    expect_that!(created?.body.len(), eq(0));
    Ok((vocabulary, registration.id().cloned().or_fail()?, cleanup))
}

/// Retrieves the entity type of the registration's first information element with `reader`.
async fn retrieved_type(reader: &BrokerClient, id: &RegistrationId) -> Result<Value> {
    let received = reader.with_accept(Accept::any()).with_responses().registrations().retrieve(id).send().await;
    json_body(&received?)?.pointer("/information/0/entities/0/type").cloned().or_fail()
}

/// Pins: Check that the @context is obtained from the request payload body itself if the Content-Type header is
/// "application/ld+json" and retrieve the information with ngsild context
/// Clause: 6.3.5
/// Isolation: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it
/// matches only the test's own entities
#[gtest]
#[tokio::test]
async fn e033_07_01_payload_context_with_json_ld_content_type_retrieved_with_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let (vocabulary, id, _registration) = created(&broker, &namespace).await?;

    let reader = broker.with_context(vocabulary.context())?;
    expect_that!(retrieved_type(&reader, &id).await?, eq(&json!("Vehicle")));

    broker.registrations().delete(&id).send().await?;
    Ok(())
}

/// Pins: Check that the @context is obtained from the request payload body itself if the Content-Type header is
/// "application/ld+json" and retrieve the information without ngsild context
/// Clause: 6.3.5
/// Isolation: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it
/// matches only the test's own entities
#[gtest]
#[tokio::test]
async fn e033_07_02_payload_context_with_json_ld_content_type_retrieved_without_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let (_, id, _registration) = created(&broker, &namespace).await?;

    expect_that!(retrieved_type(&broker, &id).await?, eq(&json!(namespace.expanded_term("Vehicle")?)));

    broker.registrations().delete(&id).send().await?;
    Ok(())
}
