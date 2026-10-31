//! Join skips a target that resolves to no entity.

use andromeda_test_suite::{
    attribute_matcher::{property, relationship},
    broker,
    core_context::core_context,
    entity_matcher::{has_attribute, member_count},
    identity_matcher::has_id,
    linked_entity::{linked_entities, relationship_objects},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::InlineJoin},
    model::{Entity, NgsiLdMediaType, OneOrMany},
};

/// Pins: Retrieve Entity with a join skips a Relationship target that resolves to no entity: inline leaves the
/// Relationship without an entity member and nests only the targets that resolve, flat answers only what resolves,
/// as JSON and as JSON-LD
/// Clause: 5.7.1.4, 4.5.23.1
#[gtest]
#[tokio::test]
async fn n_lnk_ret_0007_join_skips_a_target_that_resolves_to_no_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let building_1_id = namespace.urn("Building", 1)?;
    let city_missing_id = namespace.urn("City", "missing")?;
    let building_alice_id = namespace.urn("Building", "alice")?;
    let building_root_id = namespace.urn("Building", "root")?;
    let building_missing_id = namespace.urn("Building", "missing")?;
    let entities = [
        Entity::builder()
            .id(building_1_id.clone())
            .entity_type(building_type.clone())
            .relationship("locatedAt", city_missing_id.clone())
            .build()?,
        Entity::builder()
            .id(building_alice_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Alice")
            .build()?,
        Entity::builder()
            .id(building_root_id.clone())
            .entity_type(building_type.clone())
            .relationship("linkedTo", vec![building_missing_id.clone(), building_alice_id.clone()])
            .relationship("hasNeighbours", vec![building_missing_id.clone()])
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // the entity unchanged, no entity member
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_1_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(member_count(entity), eq(3));
    expect_that!(entity, has_id(&building_1_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(entity, has_attribute("locatedAt", relationship().object(city_missing_id.clone())));
    expect_that!(linked_entities(entity, &["locatedAt"]), is_empty());
    // a one-element array holding the Building only
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&building_1_id)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entities_2 = retrieved_2.body().as_slice();
    expect_that!(entities_2, len(eq(1)));
    expect_that!(entities_2.first().map(member_count), some(eq(3)));
    expect_that!(entities_2, nth(0, has_id(&building_1_id)));
    expect_that!(entities_2, nth(0, has_attribute("locatedAt", relationship().object(city_missing_id.clone()))));
    // of two targets only the one that resolves is nested; a Relationship none of whose targets resolve gains no entity member
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_root_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    let nested = linked_entities(entity_3, &["linkedTo"]);
    expect_that!(nested, len(eq(1)));
    expect_that!(nested, nth(0, has_id(&building_alice_id)));
    expect_that!(nested, nth(0, has_attribute("name", property().value("Alice"))));
    expect_that!(linked_entities(entity_3, &["hasNeighbours"]), is_empty());
    expect_that!(relationship_objects(entity_3, "hasNeighbours"), contains(eq(&building_missing_id)));
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&building_1_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entity_4 = retrieved_4.body();
    expect_that!(entity_4, has_id(&building_1_id));
    expect_that!(entity_4.context, some(eq(&core_context()?)));
    expect_that!(entity_4, has_attribute("locatedAt", relationship().object(city_missing_id.clone())));
    expect_that!(linked_entities(entity_4, &["locatedAt"]), is_empty());
    let retrieved_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve_flattened(&building_1_id)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entities_5 = retrieved_5.body().as_slice();
    expect_that!(entities_5, len(eq(1)));
    expect_that!(entities_5, nth(0, has_id(&building_1_id)));
    Ok(())
}
