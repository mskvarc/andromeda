//! ETSI `049_02`: a GET whose `Accept` field is `application/geo+json` is refused with 406 by an operation that does
//! not answer it (clause 6.3.4).
//!
//! The targets are written as the ETSI keywords write them and the `Accept` field is the case's subject, so each
//! request is built by `request_to` rather than by an operation, which would write its own `Accept`.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    wire::{content_type, request_to},
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};

/// Pins: Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header is
/// "application/geo+json" for operations different than "Retrieve Entity" and "Query Entity" (get
/// /subscriptions/{subscriptionId})
/// Clause: 6.3.4
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e049_02_01_retrieve_subscription_by_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id = namespace.urn_text("Subscription", 1);
    let request = request_to(&broker, Method::GET, &format!("subscriptions/{id}"))?;
    let request = content_type(request, "application/json").append_header(ACCEPT, HeaderValue::from_static("application/geo+json"));
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NOT_ACCEPTABLE));
    Ok(())
}

/// Pins: Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header is
/// "application/geo+json" for operations different than "Retrieve Entity" and "Query Entity" (get /temporal/entities)
/// Clause: 6.3.4
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e049_02_02_query_temporal_entities() -> Result<()> {
    let broker = broker::front()?;
    let request = request_to(&broker, Method::GET, "temporal/entities?type=Vehicle&timerel=after&timeAt=2020-08-01T12:05:00Z")?
        .append_header(ACCEPT, HeaderValue::from_static("application/geo+json"));
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NOT_ACCEPTABLE));
    Ok(())
}

/// Pins: Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header is
/// "application/geo+json" for operations different than "Retrieve Entity" and "Query Entity" (get
/// /csourceRegistrations)
/// Clause: 6.3.4
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e049_02_03_query_context_source_registration() -> Result<()> {
    let broker = broker::front()?;
    let request =
        request_to(&broker, Method::GET, "csourceRegistrations?type=Building")?.append_header(ACCEPT, HeaderValue::from_static("application/geo+json"));
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NOT_ACCEPTABLE));
    Ok(())
}
