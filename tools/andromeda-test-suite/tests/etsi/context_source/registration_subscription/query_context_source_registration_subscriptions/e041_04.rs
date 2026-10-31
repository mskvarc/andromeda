//! A query with a negative or zero `limit` or a negative `offset` is refused with 400 `BadRequestData` (clause 5.11.5).

use crate::unchecked_requests::refused_get;
use andromeda_test_suite::broker;
use googletest::prelude::*;

/// Pins: Check that one cannot query context source registration subscriptions with invalid offset and limit
/// parameters
/// Clause: 5.11.5
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e041_04_01_invalid_limit() -> Result<()> {
    refused_get(&broker::front()?, "csourceSubscriptions?limit=-5&offset=2").await
}

/// Pins: Check that one cannot query context source registration subscriptions with invalid offset and limit
/// parameters
/// Clause: 5.11.5
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e041_04_02_invalid_offset() -> Result<()> {
    refused_get(&broker::front()?, "csourceSubscriptions?limit=2&offset=-3").await
}

/// Pins: Check that one cannot query context source registration subscriptions with invalid offset and limit
/// parameters
/// Clause: 5.11.5
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e041_04_03_invalid_limit_and_offset() -> Result<()> {
    refused_get(&broker::front()?, "csourceSubscriptions?limit=0&offset=-1").await
}
