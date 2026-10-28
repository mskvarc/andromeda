//! ETSI `036_03`: a context source registration retrieved without a JSON-LD context is answered with its terms
//! expanded under the core context (clause 5.10.1).

use crate::suite_keywords::{RetrievalContext, expect_vehicle_parking_retrieval};
use andromeda_test_suite::namespace::Namespace;
use googletest::prelude::*;

/// Pins: Check that one can retrieve a Context Source Registration. Term to URI expansion of Attribute names shall be
/// observed.
/// Clause: 5.10.1
/// Corrected: `fix(036_03_05)`: expand isparked in the relationshipnames expectation (5.5.7)
/// Changed: availableSpotNumber, totalSpotNumber, isNextToBuilding stay out of the file vocabulary: the case reads
/// them without a @context, so they expand under the core default vocabulary as in the Robot run
/// Changed: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it matches
/// only the test's own entities.
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e036_03_01_retrieve_context_source_registration_with_default_core_context() -> Result<()> {
    let terms = ["Vehicle", "brandName", "speed", "isParked", "OffStreetParking"];
    expect_vehicle_parking_retrieval(&Namespace::fresh(), &terms, RetrievalContext::Default).await
}
