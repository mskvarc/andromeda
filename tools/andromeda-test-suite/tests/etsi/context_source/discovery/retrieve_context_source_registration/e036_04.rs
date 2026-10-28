//! ETSI `036_04`: a context source registration is retrieved by its id in the context it was created with (clause
//! 5.10.1).

use crate::{
    suite_keywords::{RetrievalContext, expect_vehicle_parking_retrieval},
    suite_payloads::VEHICLE_PARKING_TERMS,
};
use andromeda_test_suite::namespace::Namespace;
use googletest::prelude::*;

/// Pins: Check that one can retrieve a Context Source Registration
/// Clause: 5.10.1
/// Changed: each idPattern is prefixed with the test's namespace (`urn:ngsi-ld:OffStreetParking:<ns>-`), so it matches
/// only the test's own entities.
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e036_04_01_retrieve_context_source_registration() -> Result<()> {
    expect_vehicle_parking_retrieval(&Namespace::fresh(), &VEHICLE_PARKING_TERMS, RetrievalContext::Link).await
}
