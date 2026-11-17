//! A temporal follow up changing last n is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, problem::problem, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities by type under an `EntityMap` that the same query before
/// 2030-01-01T00:00:00Z with `lastN` 2 created answers 200 after 2020-01-01T00:00:00Z with `lastN` 2, and 400
/// `BadRequestData` with `lastN` 1
/// Clause: 5.5.14, 5.7.4.3
/// Recorded: clause 5.5.14 excepts the temporal query of a temporal request, and clause 5.7.4.3 lists `lastN` as an
/// input beside the temporal query of clause 4.11, so a request under the map repeats it
#[gtest]
#[tokio::test]
async fn n_map_qte_9011_a_temporal_follow_up_changing_last_n_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at("2024-01-01T12:00:00Z".parse()?))
        .property("speed", Property::builder(60).observed_at("2024-01-01T13:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?).last_n(2))
        .entity_map(&EntityMapRequest::returned())
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.entity_map().valid().or_fail()?.clone();
    let entity_map_request = EntityMapRequest::reuse(map.clone());
    broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?).last_n(2))
        .entity_map(&entity_map_request)
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::OK)?;
    let answer = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .with_responses()
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?).last_n(1))
        .entity_map(&entity_map_request)
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    broker.entity_maps().delete(&map.id().or_fail()?).send().await?;
    Ok(())
}
