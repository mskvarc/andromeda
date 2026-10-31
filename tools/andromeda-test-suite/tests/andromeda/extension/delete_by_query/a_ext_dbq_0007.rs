//! A window naming no interval is bad request data and removes nothing.

use andromeda_test_suite::{broker, evolution_matcher::has_evolution, namespace::Namespace, outcome::Upserted, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: a temporal delete-by-query with timerel before and an `endTimeAt`, or a between whose `endTimeAt` equals
/// its `timeAt`, answers 400 `BadRequestData` naming `endTimeAt`, and the history it named is still there
/// Clause: 4.11, 6.18.3
/// Recorded: Andromeda extension, not v1.9.1: the `temporal_delete_by_query` extension reads its window with the
/// clause 4.11 rules of the temporal query
#[gtest]
#[tokio::test]
async fn a_ext_dbq_0007_a_window_naming_no_interval_is_bad_request_data_and_removes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(1).observed_at("2026-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // No operation of the specification deletes temporal evolutions by a query, so the request is sent as written.
    let request = request_to(
        &broker,
        Method::DELETE,
        &format!("temporal/entities?type=Vehicle_{namespace}&timerel=before&timeAt=2030-01-01T00%3A00%3A00Z&endTimeAt=2030-02-01T00%3A00%3A00Z"),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("endTimeAt")
    );
    // No operation of the specification deletes temporal evolutions by a query, so the request is sent as written.
    let request_2 = request_to(
        &broker,
        Method::DELETE,
        &format!("temporal/entities?type=Vehicle_{namespace}&timerel=between&timeAt=2026-01-01T00%3A00%3A00Z&endTimeAt=2026-01-01T00%3A00%3A00Z"),
    )?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("endTimeAt")
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", len(eq(1))));
    Ok(())
}
