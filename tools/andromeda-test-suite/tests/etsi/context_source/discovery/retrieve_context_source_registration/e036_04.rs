//! A context source registration is retrieved by its id in the context it was created with (clause 5.10.1).

use crate::{
    registration_operations::{RetrievalContext, expect_vehicle_parking_retrieval},
    registration_payloads::VEHICLE_PARKING_TERMS,
};
use andromeda_test_suite::namespace::Namespace;
use googletest::prelude::*;

/// Pins: Check that one can retrieve a Context Source Registration
/// Clause: 5.10.1
/// Isolation: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it
/// matches only the test's own entities
#[gtest]
#[tokio::test]
async fn e036_04_01_retrieve_context_source_registration() -> Result<()> {
    expect_vehicle_parking_retrieval(&Namespace::fresh(), &VEHICLE_PARKING_TERMS, RetrievalContext::Link).await
}
