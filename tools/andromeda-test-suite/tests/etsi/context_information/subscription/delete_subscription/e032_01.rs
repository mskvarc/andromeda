//! ETSI `032_01`: a delete whose target names no subscription, or is not a valid URI, is refused (clause 5.8.5).

use crate::suite_keywords::{answered_with, refused};
use andromeda_test_suite::{broker, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Check that one cannot delete a subscription: If the subscription Id is not present or it is not a valid URI,
/// then an error shall be raised
/// Clause: 5.8.5
#[gtest]
#[tokio::test]
async fn e032_01_01_not_present_id() -> Result<()> {
    let broker = broker::front()?;
    let request = request_to(&broker, Method::DELETE, "subscriptions/")?;
    answered_with(&broker, request, StatusCode::METHOD_NOT_ALLOWED).await
}

/// Pins: Check that one cannot delete a subscription: If the subscription Id is not present or it is not a valid URI,
/// then an error shall be raised
/// Clause: 5.8.5
#[gtest]
#[tokio::test]
async fn e032_01_02_invalid_id() -> Result<()> {
    let broker = broker::front()?;
    let request = request_to(&broker, Method::DELETE, "subscriptions/InvalidUri")?;
    refused(&broker, request, ErrorType::BadRequestData).await
}
