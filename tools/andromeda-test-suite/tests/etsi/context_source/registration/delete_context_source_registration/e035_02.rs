//! A context source registration is not deleted when the request names no id, or names one that is not a valid URI
//! (clause 5.9.4).

use crate::{
    entity_operations::ld_json_body_client,
    registration_payloads::{VEHICLE_PARKING_TERMS, vehicle_parking_registration},
    unchecked_requests::refused,
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    config::SuiteConfig,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{model::ErrorType, registration::RegistrationId};

/// The case's setup: registers the initial registration in `vocabulary`'s context; answers the cleanup that deletes it.
async fn initial_registration(broker: &BrokerClient, namespace: &Namespace, vocabulary: &Vocabulary) -> Result<Cleanup> {
    let registration = vehicle_parking_registration(
        namespace,
        SuiteConfig::get()?.unreachable_origin(),
        namespace.urn("ContextSourceRegistration", 1)?,
        None,
    )?;
    let cleanup = Cleanup::of(broker).registration_of(&registration);
    ld_json_body_client(broker, vocabulary)?.registrations().register(&registration).send().await?;
    Ok(cleanup)
}

/// Pins: Check that one cannot delete a context source registration under some conditions
/// Clause: 5.9.4
/// Isolation: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it
/// matches only the test's own entities
#[gtest]
#[tokio::test]
async fn e035_02_01_delete_a_context_source_registration_if_the_id_is_not_present() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &VEHICLE_PARKING_TERMS).await?;
    let _registration = initial_registration(&broker, &namespace, &vocabulary).await?;

    let deleted = broker.send_unchecked(request_to(&broker, Method::DELETE, "csourceRegistrations/")?).await;
    verify_that!(&deleted, answered(StatusCode::METHOD_NOT_ALLOWED))?;

    let id: RegistrationId = namespace.urn_text("ContextSourceRegistration", 1).parse()?;
    let torn_down = broker.with_responses().registrations().delete(&id).send().await;
    verify_that!(&torn_down, answered(StatusCode::NO_CONTENT))?;
    Ok(())
}

/// Pins: Check that one cannot delete a context source registration under some conditions
/// Clause: 5.9.4
/// Isolation: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it
/// matches only the test's own entities
#[gtest]
#[tokio::test]
async fn e035_02_02_delete_a_context_source_registration_if_the_id_is_not_a_valid_uri() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &VEHICLE_PARKING_TERMS).await?;
    let _registration = initial_registration(&broker, &namespace, &vocabulary).await?;

    let invalid = request_to(&broker, Method::DELETE, "csourceRegistrations/invalidURI")?;
    refused(&broker, invalid, ErrorType::BadRequestData).await?;

    let id: RegistrationId = namespace.urn_text("ContextSourceRegistration", 1).parse()?;
    let torn_down = broker.with_responses().registrations().delete(&id).send().await;
    verify_that!(&torn_down, answered(StatusCode::NO_CONTENT))?;
    Ok(())
}
