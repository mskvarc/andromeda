//! A post follow up is filtered to the mapped entities.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Upserted, received_status::ExpectedStatus};
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

/// Pins: A POST temporal query under the `NGSILD-EntityMap` of a map listing two of three Entities of one type,
/// naming only the type, answers those two
/// Clause: 5.5.14, 6.24.3.1
#[gtest]
#[tokio::test]
async fn n_map_qte_9003_a_post_follow_up_is_filtered_to_the_mapped_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_scope_1_id = namespace.urn("Vehicle", "scope-1")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_scope_2_id = namespace.urn("Vehicle", "scope-2")?;
    let vehicle_scope_outside_id = namespace.urn("Vehicle", "scope-outside")?;
    let vehicle_scope_1_evolution = TemporalEntity::builder()
        .id(vehicle_scope_1_id.clone())
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
    broker.temporal().create_or_update(&vehicle_scope_1_evolution).send().await?.created()?;
    let vehicle_scope_2_evolution = TemporalEntity::builder()
        .id(vehicle_scope_2_id.clone())
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
    broker.temporal().create_or_update(&vehicle_scope_2_evolution).send().await?.created()?;
    let vehicle_scope_outside_evolution = TemporalEntity::builder()
        .id(vehicle_scope_outside_id)
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
    broker.temporal().create_or_update(&vehicle_scope_outside_evolution).send().await?.created()?;
    let entity_map_request = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .query(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_map(&entity_map_request)
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_scope_1_id.clone(), vehicle_scope_2_id.clone()])
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = page.entity_map().valid().or_fail()?;
    let entity_map_request_2 = EntityMapRequest::reuse(map.clone());
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .query(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_map(&entity_map_request_2)
        .entity_type(&vehicle_type)
        .limit(100)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&vehicle_scope_1_id)));
    expect_that!(page_2.items(), contains(has_id(&vehicle_scope_2_id)));
    Ok(())
}
