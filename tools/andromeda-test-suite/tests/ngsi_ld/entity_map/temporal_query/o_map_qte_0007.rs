//! A temporal entity map lifetime sets the expiry that far ahead.

use andromeda_test_suite::{broker, header_matcher::present, namespace::Namespace, outcome::Upserted, received_status::ExpectedStatus};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{DateTime, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities with `entityMap` true and `entityMapLifetime` PT5M creates an
/// `EntityMap` whose `expiresAt` lies more than four and at most five minutes ahead
/// Clause: 5.5.14, 5.2.39
/// Recorded: 5.5.14 makes `entityMapLifetime` a suggestion; Andromeda sets the map expiry that far ahead
#[gtest]
#[tokio::test]
async fn o_map_qte_0007_a_temporal_entity_map_lifetime_sets_the_expiry_that_far_ahead() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at("2024-01-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let plus_240: DateTime = (Utc::now() + TimeDelta::seconds(240)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let plus_301: DateTime = (Utc::now() + TimeDelta::seconds(301)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let entity_map_request = EntityMapRequest::returned_for("PT5M".parse()?);
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
    let map_id = map.id().or_fail()?;
    let map_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(&map_id)
        .send()
        .await?;
    expect_that!(map_2.expires_at.timestamp(), gt(plus_240.timestamp()));
    expect_that!(map_2.expires_at.timestamp(), lt(plus_301.timestamp()));
    Ok(())
}
