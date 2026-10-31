//! A listing of the stored `@context`s whose `details` or `kind` is not a value Table 6.29.3.2-1 defines is refused
//! with 400 `BadRequestData` (clause 5.13.3).

use andromeda_test_suite::{broker, problem::problem, wire};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::ErrorType;

/// Sends `GET jsonldContexts<query>`, hand-written because the typed listing writes only the defined values, and
/// requires 400 `BadRequestData` with a non-empty title and a non-empty detail that differs from it.
async fn refused_listing(query: &str) -> Result<()> {
    let broker = broker::front()?;
    let request = wire::request_to(&broker, Method::GET, &format!("jsonldContexts{query}"))?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_distinct_detail());
    Ok(())
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e052_04_01_list_contexts_no_details_kind_other_nothing_added() -> Result<()> {
    refused_listing("?kind=other").await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e052_04_02_list_contexts_details_other_no_kind_nothing_added() -> Result<()> {
    refused_listing("?details=other").await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e052_04_03_list_contexts_details_other_kind_other_nothing_added() -> Result<()> {
    refused_listing("?details=other&kind=other").await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e052_04_04_list_contexts_details_true_kind_other_nothing_added() -> Result<()> {
    refused_listing("?details=true&kind=other").await
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Vocabulary: none (the test is about @context storage itself and needs no vocabulary)
#[gtest]
#[tokio::test]
async fn e052_04_05_list_contexts_details_other_kind_hosted_nothing_added() -> Result<()> {
    refused_listing("?details=other&kind=Hosted").await
}
