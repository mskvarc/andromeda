//! A registration sent as `application/ld+json` with its `@context` in the body and a JSON-LD `Link` header too is
//! refused with 400 (clause 6.3.5).

use crate::{
    entity_operations::{context_link, ld_json_body_client},
    registration_payloads::{VEHICLE_PARKING_TERMS, vehicle_parking_registration},
    unchecked_requests::refused,
};
use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{http::operation::RegisterContextSource, model::ErrorType};

/// Pins: Check that an HTTP error response of type `BadRequestData` is raised if the Content-Type header is
/// "application/ld+json" and a JSON-LD Link header is present in the incoming HTTP request
/// Clause: 6.3.5
/// Isolation: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it
/// matches only the test's own entities
#[gtest]
#[tokio::test]
async fn e033_09_01_link_header_with_json_ld_content_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &VEHICLE_PARKING_TERMS).await?;
    let registration = vehicle_parking_registration(
        &namespace,
        SuiteConfig::get()?.unreachable_origin(),
        namespace.urn("ContextSourceRegistration", 1)?,
        None,
    )?;
    // The `ld+json` request already carries its `@context` in the body; the case adds the `Link` the delivery forbids.
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let _registration = Cleanup::of(&client).registration_of(&registration);
    let request = client
        .unchecked(&RegisterContextSource { registration: &registration })?
        .append_header(LINK, context_link(&vocabulary)?);
    refused(&broker, request, ErrorType::BadRequestData).await
}
