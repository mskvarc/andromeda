//! ETSI `034_03`: an update of a context source registration the broker does not hold is refused with 404 (clause
//! 5.9.3).

use crate::{
    suite_keywords::{csource_endpoint, ld_json_body_client, update_registration_request},
    suite_payloads::{VEHICLE_PARKING_TERMS, vehicle_parking_registration},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::UncheckedDocument, model::ErrorType};

/// Pins: Check that one cannot update a context source registration by id if the id is not known to the system
/// Clause: 5.9.3
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112 clause 4
/// has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code the
/// phrase names
/// Changed: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it matches
/// only the test's own entities.
#[gtest]
#[tokio::test]
async fn e034_03_01_update_a_context_source_registration_by_id_if_the_id_is_not_known_to_the_system() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &VEHICLE_PARKING_TERMS).await?;
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let registration = vehicle_parking_registration(&namespace, &endpoint, namespace.urn("Vehicle", "A456")?, None)?;
    let writer = ld_json_body_client(&broker, &vocabulary)?;
    // The body is the whole registration, its `id` included, which no fragment writes.
    let document = UncheckedDocument::try_from(registration.render())?;
    let request = update_registration_request(&writer, registration.id().or_fail()?, &document)?;
    let answer = writer.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
