//! A context source registration whose id another registration already holds is refused with 409 (clause 5.9.2).

use crate::{
    entity_operations::ld_json_body_client,
    registration_operations::send_registration,
    registration_payloads::{VEHICLE_PARKING_TERMS, vehicle_parking_registration},
};
use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, problem::problem, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::ErrorType;

/// Pins: Check that one cannot create a context source registration that already exists
/// Clause: 5.9.2
/// Isolation: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it
/// matches only the test's own entities
#[gtest]
#[tokio::test]
async fn e033_03_01_create_a_context_source_registration_that_already_exists() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &VEHICLE_PARKING_TERMS).await?;
    let registration = vehicle_parking_registration(
        &namespace,
        SuiteConfig::get()?.unreachable_origin(),
        namespace.urn("ContextSourceRegistration", 1)?,
        None,
    )?;
    let id = registration.id().cloned().or_fail()?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _registration = Cleanup::of(&client).registration_of(&registration);
    client.registrations().register(&registration).send().await?;

    let answer = send_registration(&client, &registration).await;
    expect_that!(&answer, problem(StatusCode::CONFLICT, ErrorType::AlreadyExists).with_title());

    broker.registrations().delete(&id).send().await?;
    Ok(())
}
