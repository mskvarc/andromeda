//! A minted map is retrieved with its candidates.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    registration::EntitySource,
};

/// Pins: an `EntityMap` created on broker-a is retrieved with the same id, expiry and candidate set mapping the
/// Entity to @none
/// Clause: 5.14.1.4, 5.14.4.4, 5.2.39
#[gtest]
#[tokio::test]
async fn a_map_ret_7000_a_minted_map_is_retrieved_with_its_candidates() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 20)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
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
    expect_that!(map.entity_map.get(&vehicle_id), some(contains(eq(&EntitySource::Local))));
    let map_id = &created.body().id;
    let map_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(map_id)
        .send()
        .await?;
    expect_that!(&map_2.id, eq(map_id));
    expect_that!(&map_2.expires_at, eq(&created.body().expires_at));
    expect_that!(&map_2.entity_map, len(eq(1)));
    expect_that!(map_2.entity_map.get(&vehicle_id), some(contains(eq(&EntitySource::Local))));
    Ok(())
}
