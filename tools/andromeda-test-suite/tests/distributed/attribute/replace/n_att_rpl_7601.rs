//! An attribute held only by the peer is resource not found.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::BrokerRole,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, ErrorType, Property},
};

/// Pins: Replace Attribute on broker A of speed, for an Entity broker A holds with `brandName` only while broker B
/// holds it with speed and no registration names B, answers 404 `ResourceNotFound` and leaves broker B unchanged
/// Clause: 5.6.19.4, 4.3.6.3
#[gtest]
#[tokio::test]
async fn n_att_rpl_7601_an_attribute_held_only_by_the_peer_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .property("speed", 42)
        .build()?;
    broker_b.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .build()?;
    broker.entities().create(&vehicle_2).send().await?.done()?;
    let attribute = Attribute::from(Property::builder(99).build()?);
    let answer = broker
        .with_responses()
        .entities()
        .replace_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?, &attribute)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved = broker_b.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(42)));
    expect_that!(entity, has_attribute("brandName", any_attribute().value("Mercedes")));
    Ok(())
}
