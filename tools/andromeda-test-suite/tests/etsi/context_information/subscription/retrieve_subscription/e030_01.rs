//! ETSI `030_01`: a retrieve whose identifier is not a valid URI is refused with 400 `BadRequestData` (clause 5.8.3).

use crate::suite_keywords::refused_get;
use andromeda_test_suite::broker;
use googletest::prelude::*;

/// Pins: Check that one cannot retrieve a subscription: If the subscription Id is not present or it is not a valid URI,
/// then an error of type `BadRequestData` shall be raised
/// Clause: 5.8.3
#[gtest]
#[tokio::test]
async fn e030_01_01_invalid_id() -> Result<()> {
    refused_get(&broker::front()?, "subscriptions/InvalidUri").await
}
