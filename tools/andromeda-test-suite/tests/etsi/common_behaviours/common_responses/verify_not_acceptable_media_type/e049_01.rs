//! A GET whose `Accept` field implies neither `application/json` nor `application/ld+json` is refused with 406 (clause
//! 6.3.4).
//!
//! The targets are written as fixed paths and the `Accept` field is the test's subject, so each request is built by
//! `request_to` rather than by an operation, which would write its own `Accept`.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    wire::{content_type, request_to},
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{model::APPLICATION_JSON, subscription::SubscriptionId};

/// Pins: Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header does not imply
/// "application/json" nor "application/ld+json" (get /entities/{entityId})
/// Clause: 6.3.4
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e049_01_01_get_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id = namespace.urn("Building", 1)?;
    let request = request_to(&broker, Method::GET, &format!("entities/{id}"))?.append_header(ACCEPT, HeaderValue::from_static("application/xml"));
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NOT_ACCEPTABLE));
    broker.entities().delete(&id).send().await.any_answer();
    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header does not imply
/// "application/json" nor "application/ld+json" (get /subscriptions/{subscriptionId})
/// Clause: 6.3.4
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e049_01_02_get_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id = namespace.urn_text("Subscription", 1);
    let request = request_to(&broker, Method::GET, &format!("subscriptions/{id}"))?;
    let request = content_type(request, &APPLICATION_JSON)?.append_header(ACCEPT, HeaderValue::from_static("application/xml"));
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NOT_ACCEPTABLE));
    let subscription: SubscriptionId = id.parse()?;
    broker.subscriptions().delete(&subscription).send().await.any_answer();
    Ok(())
}

/// Pins: Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header does not imply
/// "application/json" nor "application/ld+json" (get /csourceRegistrations/)
/// Clause: 6.3.4
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e049_01_03_get_csource_registrations() -> Result<()> {
    let broker = broker::front()?;
    let request = request_to(&broker, Method::GET, "csourceRegistrations?type=Building")?.append_header(ACCEPT, HeaderValue::from_static("application/xml"));
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NOT_ACCEPTABLE));
    Ok(())
}

/// Pins: Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header does not imply
/// "application/json" nor "application/ld+json" (get /csourceSubscriptions/)
/// Clause: 6.3.4
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e049_01_04_get_csource_subscriptions() -> Result<()> {
    let broker = broker::front()?;
    let request = request_to(&broker, Method::GET, "csourceSubscriptions")?.append_header(ACCEPT, HeaderValue::from_static("application/xml"));
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NOT_ACCEPTABLE));
    Ok(())
}

/// Pins: Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header does not imply
/// "application/json" nor "application/ld+json" (get /temporal/entities)
/// Clause: 6.3.4
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e049_01_05_get_temporal_entities() -> Result<()> {
    let broker = broker::front()?;
    let request = request_to(&broker, Method::GET, "temporal/entities?type=Vehicle&timerel=after&timeAt=2020-08-01T12:05:00Z")?
        .append_header(ACCEPT, HeaderValue::from_static("application/xml"));
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NOT_ACCEPTABLE));
    Ok(())
}
