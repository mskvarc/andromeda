//! ETSI `034_01`: a context source registration updated by its id with a new endpoint is retrieved with that endpoint
//! (clause 5.9.3).

use crate::{
    suite_keywords::{csource_endpoint, ld_json_body_client},
    suite_payloads::{VEHICLE_PARKING_TERMS, vehicle_parking_registration},
};
use andromeda_test_suite::{broker, cleanup::Cleanup, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, registration::RegistrationFragment};
use serde_json::json;

/// Pins: Check that one can update a context source registration by id
/// Clause: 5.9.3
/// Changed: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it matches
/// only the test's own entities.
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e034_01_01_update_a_context_source_registration_by_id() -> Result<()> {
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

    let new_endpoint = csource_endpoint(&namespace, "my.new.csource.org:1026").await?;
    let fragment = RegistrationFragment::builder().endpoint(new_endpoint.as_str()).build()?;
    let updated = broker.with_responses().registrations().update(id, &fragment).send().await;
    verify_that!(&updated, answered(StatusCode::NO_CONTENT))?;
    expect_that!(updated?.body.len(), eq(0));

    let retrieved = broker.with_accept(Accept::any()).with_responses().registrations().retrieve(id).send().await;
    let body = json_body(&retrieved?)?;
    expect_that!(body.pointer("/endpoint"), some(eq(&json!(new_endpoint.as_str()))));

    broker.registrations().delete(id).send().await?;
    Ok(())
}
