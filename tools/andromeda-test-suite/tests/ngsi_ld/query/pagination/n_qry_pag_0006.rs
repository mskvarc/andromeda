//! A mapped entity deleted since is skipped.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: Query Entities under an `NGSILD-EntityMap` of two Entities, one deleted after the map was created, answers
/// only the other
/// Clause: 6.4.3.2, 5.14.4, 5.6.6
#[gtest]
#[tokio::test]
async fn n_qry_pag_0006_a_mapped_entity_deleted_since_is_skipped() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let entities = [
        Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
            .build()?,
        Entity::builder()
            .id(vehicle_2_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(2)));
    broker.entities().delete(&vehicle_id).send().await?.done()?;
    let map_header = created.entity_map().valid().or_fail()?;
    let entity_map_request = EntityMapRequest::reuse(map_header.clone());
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .limit(100)
        .entity_map(&entity_map_request)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_2_id)));
    Ok(())
}
