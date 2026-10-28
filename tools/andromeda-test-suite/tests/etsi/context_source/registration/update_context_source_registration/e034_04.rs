//! ETSI `034_04`: an update of a context source registration whose body is not JSON is refused with 400
//! `InvalidRequest` (clause 5.9.3).

use crate::{
    suite_keywords::{csource_endpoint, ld_json_body_client, refused, update_registration_request, with_doubled_separator},
    suite_payloads::{VEHICLE_PARKING_TERMS, a456_information, parking_information},
};
use andromeda_test_suite::{broker, cleanup::Cleanup, namespace::Namespace, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::UncheckedDocument,
    model::ErrorType,
    registration::{CSourceRegistration, RegistrationFragment, RegistrationId},
};

/// Pins: Check that one cannot update a context source registration if the request body is invalid
/// Clause: 5.9.3
/// Changed: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it matches
/// only the test's own entities.
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e034_04_01_update_a_context_source_registration_if_the_request_body_is_invalid() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &VEHICLE_PARKING_TERMS).await?;
    let registration = CSourceRegistration::builder(csource_endpoint(&namespace, "my.csource.org:1026").await?.as_str())
        .id(namespace.urn_text("ContextSourceRegistration", 2).parse::<RegistrationId>()?)
        .information([a456_information(namespace.urn("Vehicle", "A456")?)?, parking_information(&namespace)?])
        .build()?;
    let id = registration.id().or_fail()?;
    let _registration = Cleanup::of(&broker).registration_of(&registration);
    ld_json_body_client(&broker, &vocabulary)?
        .registrations()
        .register(&registration)
        .send()
        .await?;

    let new_endpoint = csource_endpoint(&namespace, "my.new.csource.org:1026").await?;
    let fragment = RegistrationFragment::builder().endpoint(new_endpoint.as_str()).build()?;
    let random = namespace.urn_text("ContextSourceRegistration", "randomUUID");
    let document = UncheckedDocument::try_from(fragment.render())?.set_text("id", &random);
    let request = with_doubled_separator(update_registration_request(&broker, id, &document)?);
    refused(&broker, request, ErrorType::InvalidRequest).await?;

    let deleted = broker.with_responses().registrations().delete(id).send().await;
    verify_that!(&deleted, answered(StatusCode::NO_CONTENT))
}
