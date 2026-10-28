//! ETSI `034_02`: a context source registration update is refused with 400 when the target is not a valid URI, when
//! the body is not a registration, or when it removes the mandatory `information` (clause 5.9.3).

use crate::{
    suite_keywords::{csource_endpoint, ld_json_body_client, update_registration_request},
    suite_payloads::{VEHICLE_PARKING_TERMS, a456_information, parking_information, polygon},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    namespace::Namespace,
    problem::{answered, problem},
    vocabulary::Vocabulary,
    wire::api_uri,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::UncheckedDocument,
    model::{ErrorType, Input, RawJson, geometry::Shape},
    registration::{CSourceRegistration, RegistrationId},
};
use url::Url;

/// The initial registration of the cases, `urn:ngsi-ld:ContextSourceRegistration:<ns>-1`: the Vehicle
/// `urn:ngsi-ld:Vehicle:<ns>-A456` and the `OffStreetParking` patterns, at `endpoint`, located at `location` when it
/// names one.
fn initial_registration(namespace: &Namespace, endpoint: &Url, location: Option<Shape>) -> Result<CSourceRegistration<Input>> {
    Ok(CSourceRegistration::builder(endpoint.as_str())
        .id(namespace.urn_text("ContextSourceRegistration", 1).parse::<RegistrationId>()?)
        .information([a456_information(namespace.urn("Vehicle", "A456")?)?, parking_information(namespace)?])
        .maybe_location(location)
        .build()?)
}

/// The update body of the first two cases: the initial registration of type `building`, located in the polygon
/// 100,0 101,0 101,1 100,1.
fn building_document(namespace: &Namespace, endpoint: &Url) -> Result<UncheckedDocument> {
    let square = polygon(&[(100.0, 0.0), (101.0, 0.0), (101.0, 1.0), (100.0, 1.0), (100.0, 0.0)])?;
    let located = initial_registration(namespace, endpoint, Some(square))?;
    Ok(UncheckedDocument::try_from(located.render())?.set_text("type", "building"))
}

/// The case's setup: registers the vocabulary and the initial registration with `client`, answering the ld+json
/// client in the vocabulary's context, the registration, its endpoint and the cleanup that deletes it.
async fn created(client: &BrokerClient, namespace: &Namespace) -> Result<(BrokerClient, CSourceRegistration<Input>, Url, Cleanup)> {
    let vocabulary = Vocabulary::register(client, namespace, &VEHICLE_PARKING_TERMS).await?;
    let writer = ld_json_body_client(client, &vocabulary)?;
    let endpoint = csource_endpoint(namespace, "my.csource.org:1026").await?;
    let registration = initial_registration(namespace, &endpoint, None)?;
    let cleanup = Cleanup::of(client).registration_of(&registration);
    writer.registrations().register(&registration).send().await?;
    Ok((writer, registration, endpoint, cleanup))
}

/// Requires the teardown delete of `registration` to answer 204.
async fn torn_down(client: &BrokerClient, registration: &CSourceRegistration<Input>) -> Result<()> {
    let deleted = client.with_responses().registrations().delete(registration.id().or_fail()?).send().await;
    verify_that!(&deleted, answered(StatusCode::NO_CONTENT))
}

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
async fn e034_02_01_update_a_context_source_registration_by_id_if_the_id_is_not_a_valid_uri() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let (writer, registration, endpoint, _registration) = created(&broker, &namespace).await?;
    let document = building_document(&namespace, &endpoint)?.set_text("id", "invalidURI");
    let request = update_registration_request(&writer, registration.id().or_fail()?, &document)?.uri(api_uri(&writer, "csourceRegistrations/invalidURI")?);
    let answer = writer.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    torn_down(&broker, &registration).await
}

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
async fn e034_02_02_update_a_context_source_registration_if_the_request_body_is_not_of_the_same_data_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let (writer, registration, endpoint, _registration) = created(&broker, &namespace).await?;
    let document = building_document(&namespace, &endpoint)?;
    let request = update_registration_request(&writer, registration.id().or_fail()?, &document)?;
    let answer = writer.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    torn_down(&broker, &registration).await
}

/// Pins: Check that one cannot update a context source registration under some conditions
/// Clause: 5.9.3
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112 clause 4
/// has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code the
/// phrase names
/// Tenant: private (a broker that failed to refuse the request would hold a Context Source Registration whose
/// information was removed, which matches every other file's queries in the shared tenant (clause 5.12) and would
/// forward them; clause 4.14 keeps each tenant's registrations its own)
/// Changed: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it matches
/// only the test's own entities.
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e034_02_03_update_a_context_source_registration_if_one_attempts_to_remove_a_mandatory_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let (writer, registration, _, _registration) = created(&tenant, &namespace).await?;
    let id = registration.id().or_fail()?;
    // No typed fragment writes a null `information`, which a registration cannot lose.
    let null_information: RawJson = "null".parse()?;
    let document = UncheckedDocument::empty()
        .set_text("id", id.as_str())
        .set_text("type", "ContextSourceRegistration")
        .set_member("information", &null_information);
    let answer = writer.send_unchecked(update_registration_request(&writer, id, &document)?).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    torn_down(&tenant, &registration).await
}
