//! A GET whose `Accept` field is `application/geo+json` is refused with 406 by an operation that does not answer it
//! (clause 6.3.4).
//!
//! The `Accept` field is the test's subject and names a media type these operations never accept, so it replaces the
//! one each typed operation writes.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    wire::{accept, content_type},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::{QueryRegistrations, QueryTemporalEvolution, RetrieveSubscription},
    model::{APPLICATION_JSON, Lossless},
    query::{builders::SelectorTypeBuilder, temporal_query, type_selection},
    subscription::EntitySubscription,
};

/// Pins: Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header is
/// "application/geo+json" for operations different than "Retrieve Entity" and "Query Entity" (get
/// /subscriptions/{subscriptionId})
/// Clause: 6.3.4
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e049_02_01_retrieve_subscription_by_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id = namespace.subscription_id(1)?;
    let retrieval = RetrieveSubscription::builder().id(&id).kind(EntitySubscription).build();
    // The GET carries a Content-Type and asks for GeoJSON, which no typed operation writes, so both headers are set.
    let request = accept(content_type(broker.unchecked(&retrieval)?, &APPLICATION_JSON)?, "application/geo+json");
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NOT_ACCEPTABLE));
    Ok(())
}

/// Pins: Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header is
/// "application/geo+json" for operations different than "Retrieve Entity" and "Query Entity" (get /temporal/entities)
/// Clause: 6.3.4
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e049_02_02_query_temporal_entities() -> Result<()> {
    let broker = broker::front()?;
    let vehicle_type = SelectorTypeBuilder::from("Vehicle").build()?;
    let window = temporal_query::after("2020-08-01T12:05:00Z".parse()?).build()?;
    let query = QueryTemporalEvolution::builder()
        .entity_type(&vehicle_type)
        .temporal_q(&window)
        .format(Lossless::Normalized)
        .build();
    // The temporal query asks for GeoJSON, which no typed operation writes, so the Accept is replaced.
    let request = accept(broker.unchecked(&query)?, "application/geo+json");
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NOT_ACCEPTABLE));
    Ok(())
}

/// Pins: Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header is
/// "application/geo+json" for operations different than "Retrieve Entity" and "Query Entity" (get
/// /csourceRegistrations)
/// Clause: 6.3.4
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e049_02_03_query_context_source_registration() -> Result<()> {
    let broker = broker::front()?;
    let building_type = type_selection::entity_type("Building").build()?;
    let query = QueryRegistrations::builder().entity_type(&building_type).build();
    // The registration query asks for GeoJSON, which no typed operation writes, so the Accept is replaced.
    let request = accept(broker.unchecked(&query)?, "application/geo+json");
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NOT_ACCEPTABLE));
    Ok(())
}
