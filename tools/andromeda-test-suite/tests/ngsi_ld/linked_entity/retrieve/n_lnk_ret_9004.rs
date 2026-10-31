//! A bare and an array object on one entity each nest their own shape.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    linked_entity::{linked_entities, linked_entity_alone},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::InlineJoin},
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Retrieve Entity with join inline of an Entity holding a Relationship with one bare target and another with
/// two targets nests one Entity object under the first and both targets under the second, and join flat answers all
/// four Entities
/// Clause: 4.5.23.2, 4.5.23.3, 4.5.3.2
#[gtest]
#[tokio::test]
async fn n_lnk_ret_9004_a_bare_and_an_array_object_on_one_entity_each_nest_their_own_shape() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let building_a_id = namespace.urn("Building", "mixed-a")?;
    let building_b_id = namespace.urn("Building", "mixed-b")?;
    let building_c_id = namespace.urn("Building", "mixed-c")?;
    let building_root_id = namespace.urn("Building", "mixed-root")?;
    let building_a = Entity::builder()
        .id(building_a_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Alice")
        .build()?;
    broker.entities().create(&building_a).send().await?.done()?;
    let building_b = Entity::builder()
        .id(building_b_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Bob")
        .build()?;
    broker.entities().create(&building_b).send().await?.done()?;
    let building_c = Entity::builder()
        .id(building_c_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Carol")
        .build()?;
    broker.entities().create(&building_c).send().await?.done()?;
    let building_root = Entity::builder()
        .id(building_root_id.clone())
        .entity_type(building_type.clone())
        .relationship("linkedTo", building_c_id.clone())
        .relationship("hasNeighbours", vec![building_a_id.clone(), building_b_id.clone()])
        .build()?;
    broker.entities().create(&building_root).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_root_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(linked_entity_alone(entity, &["linkedTo"]), eq(true));
    expect_that!(linked_entities(entity, &["linkedTo"]), nth(0, has_id(&building_c_id)));
    expect_that!(linked_entity_alone(entity, &["hasNeighbours"]), eq(false));
    let neighbours = linked_entities(entity, &["hasNeighbours"]);
    expect_that!(neighbours, len(eq(2)));
    expect_that!(neighbours, contains(has_id(&building_a_id)));
    expect_that!(neighbours, contains(has_id(&building_b_id)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&building_root_id)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entities_2 = retrieved_2.body().as_slice();
    expect_that!(entities_2, len(eq(4)));
    expect_that!(entities_2, contains(has_id(&building_a_id)));
    expect_that!(entities_2, contains(has_id(&building_b_id)));
    expect_that!(entities_2, contains(has_id(&building_c_id)));
    expect_that!(entities_2, contains(has_id(&building_root_id)));
    Ok(())
}
