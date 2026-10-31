//! Inline nests each linked entity.

use andromeda_test_suite::{
    attribute_matcher::relationship,
    broker,
    entity_lookup::with_id,
    entity_matcher::{has_attribute, member_count, projected_member_count},
    identity_matcher::has_id,
    linked_entity::linked_entities,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    simplified_matcher::{has_value, simplified_member_count},
    value::object,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Entity, NgsiLdMediaType, OneOrMany, Relationship, Value, VocabTerm},
    query::Join,
};

/// Pins: Query Entities with join=inline answers only the matching entities, each Relationship carrying its Linked
/// Entity in an entity sub-property, in normalized and in simplified representation
/// Clause: 4.5.23.2, 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_lnk_qry_0001_inline_nests_each_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let building_1_id = namespace.urn("Building", 1)?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_x_id = namespace.urn("Building", "x")?;
    let city_type = namespace.entity_type("City")?;
    let city_1_id = namespace.urn("City", 1)?;
    let city_2_id = namespace.urn("City", 2)?;
    let city_x_id = namespace.urn("City", "x")?;
    let entities = [
        Entity::builder()
            .id(building_1_id.clone())
            .entity_type(building_type.clone())
            .relationship("locatedAt", Relationship::builder(city_1_id.clone()).object_type(VocabTerm::from(&city_type)))
            .build()?,
        Entity::builder().id(city_1_id.clone()).entity_type(city_type.clone()).build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .relationship("locatedAt", Relationship::builder(city_2_id.clone()).object_type(VocabTerm::from(&city_type)))
            .build()?,
        Entity::builder().id(city_2_id.clone()).entity_type(city_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // Normalized.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), not(contains(has_id(&city_1_id))));
    expect_that!(page.items(), not(contains(has_id(&city_2_id))));
    for (building_id, city_id) in [(&building_1_id, &city_1_id), (&building_2_id, &city_2_id)] {
        let building = with_id(page.items(), building_id).or_fail()?;
        expect_that!(member_count(building), eq(3));
        expect_that!(building.entity_type, eq(&OneOrMany::one(building_type.clone())));
        expect_that!(
            building,
            has_attribute("locatedAt", relationship().object(city_id.clone()).object_type(VocabTerm::from(&city_type)))
        );
        let cities = linked_entities(building, &["locatedAt"]);
        expect_that!(cities.first().map(projected_member_count), some(eq(2)));
        expect_that!(cities, nth(0, has_id(city_id)));
        expect_that!(cities.first().map(|city| &city.entity_type), some(eq(&Some(OneOrMany::one(city_type.clone())))));
    }
    // Simplified; the Relationship becomes the key-value form of its Linked Entity.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_simplified()
        .entity_type(&building_type)
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    for (building_id, city_id) in [(&building_1_id, &city_1_id), (&building_2_id, &city_2_id)] {
        let building = with_id(page_2.items(), building_id).or_fail()?;
        expect_that!(simplified_member_count(building), eq(3));
        expect_that!(building.entity_type, eq(&OneOrMany::one(building_type.clone())));
        expect_that!(
            building,
            has_value(
                "locatedAt",
                object([("id", Value::from(city_id.to_string())), ("type", Value::from(city_type.as_str()))])
            )
        );
    }
    // A Relationship without objectType to a locally stored entity is inlined too (4.5.23.1).
    let entities_2 = [
        Entity::builder().id(city_x_id.clone()).entity_type(city_type.clone()).build()?,
        Entity::builder()
            .id(building_x_id.clone())
            .entity_type(building_type.clone())
            .relationship("locatedAt", city_x_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities_2).send().await?.done()?;
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_x_id.clone()])
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&building_x_id)));
    expect_that!(page_3.items(), nth(0, has_attribute("locatedAt", relationship().object(city_x_id.clone()))));
    let cities_x = page_3.items().first().map(|building| linked_entities(building, &["locatedAt"])).or_fail()?;
    expect_that!(cities_x, nth(0, has_id(&city_x_id)));
    expect_that!(
        cities_x.first().map(|city| &city.entity_type),
        some(eq(&Some(OneOrMany::one(city_type.clone()))))
    );
    Ok(())
}
