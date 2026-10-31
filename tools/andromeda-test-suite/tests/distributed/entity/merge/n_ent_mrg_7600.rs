//! An entity held only by the peer is resource not found.

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
    model::{Entity, EntityFragment, ErrorType},
};

/// Pins: Merge Entity on broker A of an Entity held only by broker B, with no registration naming B, answers 404
/// `ResourceNotFound` and leaves broker B unchanged
/// Clause: 5.6.17.4, 4.3.6.3
#[gtest]
#[tokio::test]
async fn n_ent_mrg_7600_an_entity_held_only_by_the_peer_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("brandName", "Mercedes")
        .property("speed", 42)
        .build()?;
    broker_b.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder().property("speed", 99).build()?;
    let answer = broker.with_responses().entities().merge(&vehicle_id, &fragment).send().await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved = broker_b.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(42)));
    expect_that!(entity, has_attribute("brandName", any_attribute().value("Mercedes")));
    Ok(())
}
