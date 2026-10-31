//! A post temporal query creates and reports its map.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{EntityMapRequest, QueryMethod},
    },
    model::{NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities through POST with `entityMap` true over two Entities answers 201 with
/// `NGSILD-EntityMap` naming a map that lists exactly those two
/// Clause: 5.5.14, 5.14.5.4, Table 6.24.3.1-1
#[gtest]
#[tokio::test]
async fn n_map_qte_9002_a_post_temporal_query_creates_and_reports_its_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_mint_1_id = namespace.urn("Vehicle", "mint-1")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_mint_2_id = namespace.urn("Vehicle", "mint-2")?;
    let vehicle_mint_1_evolution = TemporalEntity::builder()
        .id(vehicle_mint_1_id.clone())
        .entity_type(vehicle_type.clone())
        .property(
            "speed",
            Property::builder(Value::Number("0.0".parse()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("1.0".parse()?)).observed_at("2024-01-01T00:01:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_mint_1_evolution).send().await?.created()?;
    let vehicle_mint_2_evolution = TemporalEntity::builder()
        .id(vehicle_mint_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property(
            "speed",
            Property::builder(Value::Number("0.0".parse()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(Value::Number("1.0".parse()?)).observed_at("2024-01-01T00:01:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_mint_2_evolution).send().await?.created()?;
    let entity_map_request = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .query(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_map(&entity_map_request)
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_mint_1_id.clone(), vehicle_mint_2_id.clone()])
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = page.entity_map().valid().or_fail()?;
    let map_id = map.id().or_fail()?;
    let map_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(&map_id)
        .send()
        .await?;
    expect_that!(&map_2.entity_map, len(eq(2)));
    expect_that!(map_2.entity_map.get(&vehicle_mint_1_id), some(anything()));
    expect_that!(map_2.entity_map.get(&vehicle_mint_2_id), some(anything()));
    Ok(())
}
