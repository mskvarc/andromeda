//! ETSI `036_05`: a context source registration is retrieved in the core context when no `Link` header names one,
//! and in the `Link` header's context when one does (clause 6.3.5).

use crate::{
    suite_keywords::{RetrievalContext, expect_vehicle_parking_retrieval},
    suite_payloads::VEHICLE_PARKING_TERMS,
};
use andromeda_test_suite::namespace::Namespace;
use googletest::prelude::*;

/// Pins: Check that the JSON-LD @context is obtained from a Link header if present and that the default JSON-LD
/// @context is used if not present
/// Clause: 6.3.5
/// Corrected: `fix(036_03_05)`: expand isparked in the relationshipnames expectation (5.5.7)
/// Changed: availableSpotNumber, totalSpotNumber, isNextToBuilding stay out of the file vocabulary: the case reads
/// them without a @context, so they expand under the core default vocabulary as in the Robot run
/// Changed: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it matches
/// only the test's own entities.
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e036_05_01_empty_json_ld_context() -> Result<()> {
    let terms = ["Vehicle", "brandName", "speed", "isParked", "OffStreetParking"];
    expect_vehicle_parking_retrieval(&Namespace::fresh(), &terms, RetrievalContext::Default).await
}

/// Pins: Check that the JSON-LD @context is obtained from a Link header if present and that the default JSON-LD
/// @context is used if not present
/// Clause: 6.3.5
/// Changed: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it matches
/// only the test's own entities.
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e036_05_02_creation_time_json_ld_context() -> Result<()> {
    expect_vehicle_parking_retrieval(&Namespace::fresh(), &VEHICLE_PARKING_TERMS, RetrievalContext::Link).await
}
