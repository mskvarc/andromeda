//! An update whose subscription identifier is missing or is not a valid URI is refused (clause 5.8.2).

use crate::unchecked_requests::{answered_with, refused};
use andromeda_test_suite::{broker, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Pins: Check that one cannot update a subscription: If the Subscription id is not present or it is not a valid URI,
/// then an error of type `BadRequestData` shall be raised
/// Clause: 5.8.2
#[gtest]
#[tokio::test]
async fn e029_01_01_not_present_id() -> Result<()> {
    let broker = broker::front()?;
    let request = request_to(&broker, Method::PATCH, "subscriptions/")?;
    answered_with(&broker, request, StatusCode::METHOD_NOT_ALLOWED).await
}

/// Pins: Check that one cannot update a subscription: If the Subscription id is not present or it is not a valid URI,
/// then an error of type `BadRequestData` shall be raised
/// Clause: 5.8.2
#[gtest]
#[tokio::test]
async fn e029_01_02_invalid_id() -> Result<()> {
    let broker = broker::front()?;
    let request = request_to(&broker, Method::PATCH, "subscriptions/InvalidUri")?;
    refused(&broker, request, ErrorType::BadRequestData).await
}
