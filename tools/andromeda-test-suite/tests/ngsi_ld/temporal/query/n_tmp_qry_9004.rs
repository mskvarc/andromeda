//! A partial temporal map returns only its entities.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Upserted, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities naming three ids under a temporal `EntityMap` of two of them answers
/// those two
/// Clause: 5.5.14, 5.14.5
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9004_a_partial_temporal_map_returns_only_its_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let building_evolution = TemporalEntity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property(
            "temperature",
            Property::builder(Value::Number("30.0".parse()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&building_evolution).send().await?.created()?;
    let building_2_evolution = TemporalEntity::builder()
        .id(building_2_id.clone())
        .entity_type(building_type.clone())
        .property(
            "temperature",
            Property::builder(Value::Number("30.0".parse()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&building_2_evolution).send().await?.created()?;
    let building_3_evolution = TemporalEntity::builder()
        .id(building_3_id.clone())
        .entity_type(building_type.clone())
        .property(
            "temperature",
            Property::builder(Value::Number("30.0".parse()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&building_3_evolution).send().await?.created()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .entity_type(&building_type)
        .ids(vec![building_id.clone(), building_2_id.clone()])
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(2)));
    let map_header = created.entity_map().valid().or_fail()?;
    let entity_map_request = EntityMapRequest::reuse(map_header.clone());
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .entity_map(&entity_map_request)
        .entity_type(&building_type)
        .ids(vec![building_id.clone(), building_2_id.clone(), building_3_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&building_id)));
    expect_that!(page.items(), contains(has_id(&building_2_id)));
    let map_id = &created.body().id;
    broker.entity_maps().delete(map_id).send().await?;
    Ok(())
}
