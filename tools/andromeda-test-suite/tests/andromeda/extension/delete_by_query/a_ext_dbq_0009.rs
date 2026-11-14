//! A q crossing a Relationship is bad request data and removes nothing.

use andromeda_test_suite::{broker, evolution_matcher::has_evolution, namespace::Namespace, outcome::Upserted, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: a temporal delete-by-query whose q is parkedIn{free}>0 answers 400 `BadRequestData` naming the delete and
/// the Linked Entity its q traverses, and the history it named is still there
/// Clause: 4.9, 5.6.21.4, 5.7.4.4
/// Recorded: Andromeda extension, not v1.9.1: the `temporal_delete_by_query` extension takes the entity selection of
/// a temporal query and no `join`, so a hop of its q is refused as Purge Entities refuses one
#[gtest]
#[tokio::test]
async fn a_ext_dbq_0009_a_q_crossing_a_relationship_is_bad_request_data_and_removes_nothing() -> Result<()> {
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
        &format!("temporal/entities?type=Vehicle_{namespace}&q=parkedIn%7Bfree%7D%3E0&timerel=before&timeAt=2030-01-01T00%3A00%3A00Z"),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("temporal delete-by-query")
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
