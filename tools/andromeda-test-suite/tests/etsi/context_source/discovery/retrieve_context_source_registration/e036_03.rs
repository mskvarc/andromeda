//! A context source registration retrieved without a JSON-LD context is answered with its terms expanded under the core
//! context (clause 5.10.1).

use crate::registration_operations::{RetrievalContext, expect_vehicle_parking_retrieval};
use andromeda_test_suite::namespace::Namespace;
use googletest::prelude::*;

/// Pins: Check that one can retrieve a Context Source Registration. Term to URI expansion of Attribute names shall be
/// observed.
/// Clause: 5.10.1
/// Expected: isParked is expanded in the relationshipNames expectation (clause 5.5.7)
/// Expected: availableSpotNumber, totalSpotNumber, isNextToBuilding stay out of the file vocabulary: the test reads
/// them without a @context, so they expand under the core default vocabulary
/// Isolation: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it
/// matches only the test's own entities
#[gtest]
#[tokio::test]
async fn e036_03_01_retrieve_context_source_registration_with_default_core_context() -> Result<()> {
    let terms = ["Vehicle", "brandName", "speed", "isParked", "OffStreetParking"];
    expect_vehicle_parking_retrieval(&Namespace::fresh(), &terms, RetrievalContext::Default).await
}
