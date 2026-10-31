//! Attrs naming only attributes held by an unregistered peer is not found.

use andromeda_test_suite::{broker, config::BrokerRole, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType},
};

/// Pins: Retrieve Entity on broker A with attrs colour, of an Entity broker A holds without colour while broker B
/// holds colour and no registration names B, answers 404 `ResourceNotFound`
/// Clause: 5.7.1.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_ent_ret_7600_attrs_naming_only_attributes_held_by_an_unregistered_peer_is_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("colour", "red")
        .build()?;
    broker_b.entities().create(&vehicle_2).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .attrs(vec!["colour".parse()?])
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
