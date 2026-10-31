//! A json retrieve carries the context link and no context member.

use andromeda_test_suite::{broker, link_matcher::link_value, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::{Accept, Link, RelationType},
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: Retrieve `EntityMap` with Accept application/json answers a context Link header and a body without
/// @context
/// Clause: 6.3.5, 5.14.1.4
#[gtest]
#[tokio::test]
async fn n_map_ret_0004_a_json_retrieve_carries_the_context_link_and_no_context_member() -> Result<()> {
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
    let retrieved = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(map_id)
        .send()
        .await?;
    expect_that!(retrieved.status, eq(StatusCode::OK));
    expect_that!(retrieved.head.typed::<Link>().valid(), some(link_value(RelationType::JsonLdContext)));
    expect_that!(&retrieved.outcome.id, eq(map_id));
    expect_that!(retrieved.outcome.context.as_ref(), none());
    Ok(())
}
