//! A limit is ignored and every matching entity is mapped.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    registration::{EntityMap, EntitySource},
};

/// Pins: Create `EntityMap` for Query Entities with a limit of 1 over two matching Entities maps both, the limit
/// being a parameter the creation ignores
/// Clause: 5.14.4.3, 5.14.4.4
#[gtest]
#[tokio::test]
async fn n_map_qen_7004_a_limit_is_ignored_and_every_matching_entity_is_mapped() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let entities = [
        Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 20)
            .build()?,
        Entity::builder()
            .id(vehicle_2_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 30)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // The creation takes no limit, so the request is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entityMaps?type=Vehicle_{namespace}&limit=1"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let response = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await?;
    expect_that!(response.status(), eq(StatusCode::CREATED));
    let map = EntityMap::parse(str::from_utf8(response.body())?)?;
    expect_that!(&map.entity_map, len(eq(2)));
    expect_that!(map.entity_map.get(&vehicle_id), some(contains(eq(&EntitySource::Local))));
    expect_that!(map.entity_map.get(&vehicle_2_id), some(contains(eq(&EntitySource::Local))));
    Ok(())
}
