//! Flat appends the linked entities.

use andromeda_test_suite::{
    attribute_matcher::relationship,
    broker,
    entity_lookup::with_id,
    entity_matcher::{has_attribute, member_count},
    identity_matcher::ids_in_order,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    simplified_matcher::{has_value, simplified_member_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Entity, NgsiLdMediaType, OneOrMany, Relationship, VocabTerm},
    query::Join,
};

/// Pins: Query Entities with join=flat answers one array holding the matching entities followed by the entities
/// their Relationships link to, in normalized and in simplified representation
/// Clause: 4.5.23.3, 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_lnk_qry_0003_flat_appends_the_linked_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let building_1_id = namespace.urn("Building", 1)?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_a_id = namespace.urn("Building", "a")?;
    let building_b_id = namespace.urn("Building", "b")?;
    let city_type = namespace.entity_type("City")?;
    let city_1_id = namespace.urn("City", 1)?;
    let city_2_id = namespace.urn("City", 2)?;
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
    // Normalized, both Buildings first, then the two Cities they link to.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(4)));
    let types: Vec<_> = page.items().iter().map(|item| item.entity_type.clone()).collect();
    expect_that!(
        types,
        elements_are![
            eq(&OneOrMany::one(building_type.clone())),
            eq(&OneOrMany::one(building_type.clone())),
            eq(&OneOrMany::one(city_type.clone())),
            eq(&OneOrMany::one(city_type.clone()))
        ]
    );
    for (building_id, city_id) in [(&building_1_id, &city_1_id), (&building_2_id, &city_2_id)] {
        let building = with_id(page.items(), building_id).or_fail()?;
        expect_that!(member_count(building), eq(3));
        expect_that!(
            building,
            has_attribute("locatedAt", relationship().object(city_id.clone()).object_type(VocabTerm::from(&city_type)))
        );
        let city = with_id(page.items(), city_id).or_fail()?;
        expect_that!(member_count(city), eq(2));
    }
    // The same array in simplified representation.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_simplified()
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(4)));
    let types_2: Vec<_> = page_2.items().iter().map(|item| item.entity_type.clone()).collect();
    expect_that!(
        types_2,
        elements_are![
            eq(&OneOrMany::one(building_type.clone())),
            eq(&OneOrMany::one(building_type.clone())),
            eq(&OneOrMany::one(city_type.clone())),
            eq(&OneOrMany::one(city_type.clone()))
        ]
    );
    for (building_id, city_id) in [(&building_1_id, &city_1_id), (&building_2_id, &city_2_id)] {
        let building = with_id(page_2.items(), building_id).or_fail()?;
        expect_that!(simplified_member_count(building), eq(3));
        expect_that!(building, has_value("locatedAt", city_id.to_string()));
        let city = with_id(page_2.items(), city_id).or_fail()?;
        expect_that!(simplified_member_count(city), eq(2));
    }
    // A Relationship without objectType to a locally stored entity is followed too (4.5.23.1); the id
    // restriction keeps the linked Building out of the matching set, so it is appended after the root.
    let entities_2 = [
        Entity::builder().id(building_b_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder()
            .id(building_a_id.clone())
            .entity_type(building_type.clone())
            .relationship("linkedTo", building_b_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities_2).send().await?.done()?;
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_a_id.clone()])
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), ids_in_order(&[building_a_id.clone(), building_b_id.clone()]));
    expect_that!(page_3.items(), nth(0, has_attribute("linkedTo", relationship().object(building_b_id.clone()))));
    expect_that!(
        page_3.items().get(1).map(|building| &building.entity_type),
        some(eq(&OneOrMany::one(building_type.clone())))
    );
    Ok(())
}
