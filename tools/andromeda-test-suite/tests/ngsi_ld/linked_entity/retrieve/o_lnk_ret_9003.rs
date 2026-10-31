//! A multi target relationship nests its targets in object order.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    linked_entity::linked_entities,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    simplified_matcher::has_value_at,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{InlineJoin, RepresentationSpelling},
    },
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Retrieve Entity with join inline and `joinLevel` 1 of an Entity whose Relationship names two targets nests
/// them in the order object names them, normalized and as key-value Entities
/// Clause: 4.5.23.2, 4.5.4, 4.5.3.2
/// Recorded: clause 4.5.3.2 makes the object of a Relationship a URI or an array of URIs and orders only the
/// `entityList` of a `ListRelationship`; Andromeda nests the targets of a Relationship in the order its object
/// names them, as the passengers of annex C clause C.2.2
#[gtest]
#[tokio::test]
async fn o_lnk_ret_9003_a_multi_target_relationship_nests_its_targets_in_object_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_order_a_id = namespace.urn("Building", "order-a")?;
    let building_type = namespace.entity_type("Building")?;
    let building_order_b_id = namespace.urn("Building", "order-b")?;
    let building_order_root_id = namespace.urn("Building", "order-root")?;
    let building_order_a = Entity::builder()
        .id(building_order_a_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Alice")
        .build()?;
    broker.entities().create(&building_order_a).send().await?.done()?;
    let building_order_b = Entity::builder()
        .id(building_order_b_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Bob")
        .build()?;
    broker.entities().create(&building_order_b).send().await?.done()?;
    let building_order_root = Entity::builder()
        .id(building_order_root_id.clone())
        .entity_type(building_type.clone())
        .relationship("linkedTo", vec![building_order_a_id.clone(), building_order_b_id.clone()])
        .build()?;
    broker.entities().create(&building_order_root).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_order_root_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(linked_entities(entity, &["linkedTo"]), nth(0, has_id(&building_order_a_id)));
    expect_that!(linked_entities(entity, &["linkedTo"]), nth(1, has_id(&building_order_b_id)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&building_order_root_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(
        entity_2,
        has_value_at("linkedTo", ["0", "id"], format!("urn:ngsi-ld:Building:{namespace}-order-a"))
    );
    expect_that!(
        entity_2,
        has_value_at("linkedTo", ["1", "id"], format!("urn:ngsi-ld:Building:{namespace}-order-b"))
    );
    Ok(())
}
