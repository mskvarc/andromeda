//! Join inline nests the linked entity under its relationship.

use andromeda_test_suite::{
    attribute_matcher::{property, relationship},
    broker,
    core_context::core_context,
    entity_matcher::{has_attribute, member_count, projected_member_count},
    identity_matcher::{has_id, ids_in_order},
    linked_entity::linked_entities,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::InlineJoin},
    model::{Entity, NgsiLdMediaType, OneOrMany, Relationship, VocabTerm},
};

/// Pins: Retrieve Entity with join=inline answers one entity whose Relationship carries the Linked Entity under
/// entity, with or without `objectType`, as JSON and as JSON-LD, while join=flat over the same entity answers both
/// entities
/// Clause: 5.7.1.4, 4.5.23.1, 4.5.23.2, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_lnk_ret_0001_join_inline_nests_the_linked_entity_under_its_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let city_type = namespace.entity_type("City")?;
    let city_paris_id = namespace.urn("City", "paris")?;
    let building_type = namespace.entity_type("Building")?;
    let building_typed_id = namespace.urn("Building", "typed")?;
    let building_b_id = namespace.urn("Building", "b")?;
    let building_a_id = namespace.urn("Building", "a")?;
    let entities = [
        Entity::builder().id(city_paris_id.clone()).entity_type(city_type.clone()).build()?,
        Entity::builder()
            .id(building_typed_id.clone())
            .entity_type(building_type.clone())
            .relationship(
                "locatedAt",
                Relationship::builder(city_paris_id.clone()).object_type(VocabTerm::from(&city_type)),
            )
            .build()?,
        Entity::builder()
            .id(building_b_id.clone())
            .entity_type(building_type.clone())
            .property("category", "commercial")
            .property("name", "Depot")
            .build()?,
        Entity::builder()
            .id(building_a_id.clone())
            .entity_type(building_type.clone())
            .relationship("linkedTo", building_b_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // a Relationship annotated with objectType nests the City
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_typed_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(member_count(entity), eq(3));
    expect_that!(entity, has_id(&building_typed_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(
        entity,
        has_attribute(
            "locatedAt",
            relationship().object(city_paris_id.clone()).object_type(VocabTerm::from(&city_type))
        )
    );
    let cities = linked_entities(entity, &["locatedAt"]);
    expect_that!(cities.first().map(projected_member_count), some(eq(2)));
    expect_that!(cities, nth(0, has_id(&city_paris_id)));
    expect_that!(cities.first().map(|city| &city.entity_type), some(eq(&Some(OneOrMany::one(city_type.clone())))));
    // a Relationship without objectType to a locally held entity: one object, the target nested in full
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_a_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(member_count(entity_2), eq(3));
    expect_that!(entity_2, has_id(&building_a_id));
    expect_that!(entity_2, has_attribute("linkedTo", relationship().object(building_b_id.clone())));
    let buildings = linked_entities(entity_2, &["linkedTo"]);
    expect_that!(buildings.first().map(projected_member_count), some(eq(4)));
    expect_that!(buildings, nth(0, has_id(&building_b_id)));
    expect_that!(
        buildings.first().map(|building| &building.entity_type),
        some(eq(&Some(OneOrMany::one(building_type.clone()))))
    );
    expect_that!(buildings, nth(0, has_attribute("category", property().value("commercial"))));
    expect_that!(buildings, nth(0, has_attribute("name", property().value("Depot"))));
    // the same as JSON-LD: one object carrying the core @context, the target nested
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&building_a_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, has_id(&building_a_id));
    expect_that!(entity_3.context, some(eq(&core_context()?)));
    expect_that!(entity_3, has_attribute("linkedTo", relationship().object(building_b_id.clone())));
    let buildings_3 = linked_entities(entity_3, &["linkedTo"]);
    expect_that!(buildings_3, nth(0, has_id(&building_b_id)));
    expect_that!(buildings_3, nth(0, has_attribute("name", property().value("Depot"))));
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&building_a_id)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entities_4 = retrieved_4.body().as_slice();
    expect_that!(entities_4, len(eq(2)));
    expect_that!(entities_4, ids_in_order(&[building_a_id.clone(), building_b_id.clone()]));
    Ok(())
}
