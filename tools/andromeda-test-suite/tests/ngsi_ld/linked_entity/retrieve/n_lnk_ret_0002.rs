//! Join flat answers the entity and its linked entity.

use andromeda_test_suite::{
    attribute_matcher::relationship,
    broker,
    entity_matcher::{has_attribute, member_count},
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, OneOrMany, Relationship, VocabTerm},
};

/// Pins: Retrieve Entity with join=flat answers an array of the entity and the Linked Entity its Relationship
/// targets, with or without `objectType` on the Relationship, as JSON and as JSON-LD
/// Clause: 5.7.1.4, 4.5.23.1, 4.5.23.3, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_lnk_ret_0002_join_flat_answers_the_entity_and_its_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let city_type = namespace.entity_type("City")?;
    let city_paris_id = namespace.urn("City", "paris")?;
    let building_type = namespace.entity_type("Building")?;
    let building_typed_id = namespace.urn("Building", "typed")?;
    let building_untyped_id = namespace.urn("Building", "untyped")?;
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
            .id(building_untyped_id.clone())
            .entity_type(building_type.clone())
            .relationship("locatedAt", city_paris_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&building_typed_id)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entities_1 = retrieved.body().as_slice();
    expect_that!(entities_1, len(eq(2)));
    expect_that!(entities_1.first().map(member_count), some(eq(3)));
    expect_that!(entities_1, nth(0, has_id(&building_typed_id)));
    expect_that!(
        entities_1.first().map(|building| &building.entity_type),
        some(eq(&OneOrMany::one(building_type.clone())))
    );
    expect_that!(
        entities_1,
        nth(
            0,
            has_attribute(
                "locatedAt",
                relationship().object(city_paris_id.clone()).object_type(VocabTerm::from(&city_type))
            )
        )
    );
    expect_that!(entities_1.get(1).map(member_count), some(eq(2)));
    expect_that!(entities_1, nth(1, has_id(&city_paris_id)));
    expect_that!(entities_1.get(1).map(|city| &city.entity_type), some(eq(&OneOrMany::one(city_type.clone()))));
    // a locally held target is retrieved without objectType too
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&building_untyped_id)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entities_2 = retrieved_2.body().as_slice();
    expect_that!(entities_2, len(eq(2)));
    expect_that!(entities_2, nth(0, has_id(&building_untyped_id)));
    expect_that!(entities_2, nth(0, has_attribute("locatedAt", relationship().object(city_paris_id.clone()))));
    expect_that!(entities_2, nth(1, has_id(&city_paris_id)));
    expect_that!(entities_2.get(1).map(|city| &city.entity_type), some(eq(&OneOrMany::one(city_type.clone()))));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve_flattened(&building_untyped_id)
        .join_level(1.try_into()?)
        .send()
        .await?;
    let entities_3 = retrieved_3.body().as_slice();
    expect_that!(entities_3, len(eq(2)));
    expect_that!(entities_3, nth(0, has_id(&building_untyped_id)));
    expect_that!(entities_3, nth(0, has_attribute("locatedAt", relationship().object(city_paris_id.clone()))));
    expect_that!(entities_3, nth(1, has_id(&city_paris_id)));
    expect_that!(entities_3.get(1).map(|city| &city.entity_type), some(eq(&OneOrMany::one(city_type.clone()))));
    Ok(())
}
