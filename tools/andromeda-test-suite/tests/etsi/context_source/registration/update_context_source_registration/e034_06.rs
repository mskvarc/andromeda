//! ETSI `034_06`: an update of a context source registration whose target names no id is refused with 405 (clause
//! 5.9.3).

use crate::{
    suite_keywords::{answered_with, csource_endpoint, ld_json_body_client, update_registration_request},
    suite_payloads::{VEHICLE_PARKING_TERMS, a456_information, parking_information, polygon},
};
use andromeda_test_suite::{broker, cleanup::Cleanup, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::UncheckedDocument,
    registration::{CSourceRegistration, RegistrationId},
};

/// Pins: Check that one cannot update a context source registration under some conditions
/// Clause: 5.9.3
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112 clause 4
/// has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code the
/// phrase names
/// Changed: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it matches
/// only the test's own entities.
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e034_06_01_update_a_context_source_registration_by_id_if_the_id_is_not_present() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &VEHICLE_PARKING_TERMS).await?;
    let writer = ld_json_body_client(&broker, &vocabulary)?;
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let registration = CSourceRegistration::builder(endpoint.as_str())
        .id(namespace.urn_text("ContextSourceRegistration", 1).parse::<RegistrationId>()?)
        .information([a456_information(namespace.urn("Vehicle", "A456")?)?, parking_information(&namespace)?])
        .build()?;
    let id = registration.id().or_fail()?;
    let _registration = Cleanup::of(&writer).registration_of(&registration);
    writer.registrations().register(&registration).send().await?;

    // The body is a located registration of type `building` with an empty id, which no typed value writes.
    let located = CSourceRegistration::builder(endpoint.as_str())
        .information([a456_information(namespace.urn("Vehicle", "A456")?)?, parking_information(&namespace)?])
        .location(polygon(&[(100.0, 0.0), (101.0, 0.0), (101.0, 1.0), (100.0, 1.0), (100.0, 0.0)])?)
        .build()?;
    let document = UncheckedDocument::try_from(located.render())?.set_text("id", "").set_text("type", "building");
    let request = update_registration_request(&writer, id, &document)?.uri(api_uri(&writer, "csourceRegistrations/")?);
    answered_with(&writer, request, StatusCode::METHOD_NOT_ALLOWED).await?;

    let deleted = broker.with_responses().registrations().delete(id).send().await;
    verify_that!(&deleted, answered(StatusCode::NO_CONTENT))
}
