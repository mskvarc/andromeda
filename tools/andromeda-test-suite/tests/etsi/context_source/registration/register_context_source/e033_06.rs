//! ETSI `033_06`: a registration whose `application/json` body carries an `@context` is refused with 400 (clause
//! 6.3.5).

use crate::{
    suite_keywords::refused,
    suite_payloads::{VEHICLE_PARKING_TERMS, vehicle_parking_registration},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    config::SuiteConfig,
    namespace::Namespace,
    vocabulary::Vocabulary,
    wire::content_type,
};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    http::{ContextDelivery, operation::RegisterContextSource},
    model::ErrorType,
};

/// Pins: Check that an HTTP error response of type `BadRequestData` is raised if the Content-Type header is
/// "application/json" and the request payload body (as JSON) contains a "@context" term
/// Clause: 6.3.5
/// Changed: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it matches
/// only the test's own entities.
#[gtest]
#[tokio::test]
async fn e033_06_01_payload_context_with_json_content_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &VEHICLE_PARKING_TERMS).await?;
    let registration = vehicle_parking_registration(
        &namespace,
        SuiteConfig::get()?.unreachable_origin(),
        namespace.urn("ContextSourceRegistration", 1)?,
        None,
    )?;
    // The case sends the body an `ld+json` request writes, but as `application/json` and with no `Link` at all: the
    // mix clause 6.3.5 forbids.
    let client: BrokerClient = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let _registration = Cleanup::of(&client).registration_of(&registration);
    let request = client.unchecked(&RegisterContextSource { registration: &registration })?.remove_header(&LINK);
    refused(&broker, content_type(request, "application/json"), ErrorType::BadRequestData).await
}
