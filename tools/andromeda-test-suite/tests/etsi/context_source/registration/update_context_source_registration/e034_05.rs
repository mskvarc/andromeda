//! A context source registration whose `expiresAt` is removed by an update is retrieved without it (clauses 5.5.8 and
//! 5.9.3).

use crate::{
    entity_operations::ld_json_body_client,
    registration_operations::csource_endpoint,
    registration_payloads::{VEHICLE_PARKING_TERMS, vehicle_parking_registration},
};
use andromeda_test_suite::{broker, cleanup::Cleanup, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    registration::{RegistrationFragment, RegistrationMember},
};

/// Pins: Check that one can update a context source registration by id
/// Clause: 5.9.3
/// Expected: expiresAt is removed with the NGSI-LD Null urn:ngsi-ld:null, which clause 5.5.8 makes the deletion of the
/// member; clause 4.5.0 rules out a JSON null ("null cannot be used as a value")
/// Isolation: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it
/// matches only the test's own entities
#[gtest]
#[tokio::test]
async fn e034_05_01_update_a_context_source_registration_to_never_expire() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &VEHICLE_PARKING_TERMS).await?;
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let registration = vehicle_parking_registration(&namespace, &endpoint, namespace.urn("ContextSourceRegistration", 1)?, None)?;
    let id = registration.id().or_fail()?;
    let _registration = Cleanup::of(&broker).registration_of(&registration);
    ld_json_body_client(&broker, &vocabulary)?
        .registrations()
        .register(&registration)
        .send()
        .await?;

    let fragment = RegistrationFragment::builder().delete(RegistrationMember::ExpiresAt).build()?;
    let updated = broker.with_responses().registrations().update(id, &fragment).send().await;
    verify_that!(&updated, answered(StatusCode::NO_CONTENT))?;
    expect_that!(updated?.body.len(), eq(0));

    let retrieved = broker.with_accept(Accept::any()).with_responses().registrations().retrieve(id).send().await;
    let body = json_body(&retrieved?)?;
    expect_that!(body.is_object(), eq(true));
    expect_that!(body.get("expiresAt"), none());

    broker.registrations().delete(id).send().await?;
    Ok(())
}
