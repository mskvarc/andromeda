//! A temporal retrieve of an entity map member echoes the map.

use andromeda_test_suite::{broker, header_matcher::present, namespace::Namespace, outcome::Upserted, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::{Accept, NgsiLdEntityMap},
        operation::EntityMapRequest,
    },
    model::{Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Retrieve Temporal Evolution of an Entity the `EntityMap` named in `NGSILD-EntityMap` holds answers 200
/// with that Entity and the same `NGSILD-EntityMap` header
/// Clause: 5.7.3.4, 5.5.14
#[gtest]
#[tokio::test]
async fn n_map_qte_0006_a_temporal_retrieve_of_an_entity_map_member_echoes_the_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at("2024-01-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let entity_map_request = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_map(&entity_map_request)
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page.entity_map(), present());
    let map = page.entity_map().valid().or_fail()?;
    let entity_map_request_2 = EntityMapRequest::reuse(map.clone());
    let headed = broker
        .with_heads()
        .with_accept(Accept::any())
        .temporal()
        .retrieve(&vehicle_id)
        .entity_map(&entity_map_request_2)
        .send()
        .await?;
    expect_that!(headed.head.typed::<NgsiLdEntityMap>().valid(), some(eq(map)));
    let retrieved = &headed.outcome;
    let evolution = retrieved.body();
    expect_that!(evolution.id, eq(&vehicle_id));
    Ok(())
}
