//! A retrieve with entity map true names the entity map in the response header.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::Entity,
};

/// Pins: Retrieve Entity with `entityMap` true answers 200 with the entity and an `NGSILD-EntityMap` header holding
/// an entity map URI
/// Clause: 6.5.3.1, 6.3.15
#[gtest]
#[tokio::test]
async fn n_ent_ret_7200_a_retrieve_with_entity_map_true_names_the_entity_map_in_the_response_header() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let entity_map_request = EntityMapRequest::returned();
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .entity_map(&entity_map_request)
        .send()
        .await?;
    expect_that!(
        retrieved.entity_map().valid().map(|map| map.as_iri_ref().as_str()),
        some(contains_regex("^urn:ngsi-ld:EntityMap:.+$"))
    );
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(5)));
    Ok(())
}
