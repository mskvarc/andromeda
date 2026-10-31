//! A json ld retrieve carries the context member.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: Retrieve `EntityMap` with Accept application/ld+json answers a body with an @context member
/// Clause: 6.3.5, 5.14.1.4
#[gtest]
#[tokio::test]
async fn n_map_ret_0005_a_json_ld_retrieve_carries_the_context_member() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let entities = [Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?];
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
    let map_id = &created.body().id;
    let map = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entity_maps()
        .retrieve(map_id)
        .send()
        .await?;
    expect_that!(&map.id, eq(map_id));
    expect_that!(map.context.as_ref(), some(anything()));
    Ok(())
}
