//! Flat follows exactly join level hops.

use andromeda_test_suite::{
    attribute_matcher::relationship,
    broker,
    entity_lookup::with_id,
    entity_matcher::{has_attribute, member_count},
    identity_matcher::ids_in_order,
    namespace::Namespace,
    outcome::Done,
    simplified_matcher::{has_value, simplified_member_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Entity, EntityId, NgsiLdMediaType, OneOrMany, Relationship, VocabTerm},
    query::Join,
};

/// Pins: Query Entities with join=flat follows Relationships exactly `joinLevel` hops deep, appending every entity
/// reached within that depth and none beyond it
/// Clause: 4.5.23.3, 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_lnk_qry_0004_flat_follows_exactly_join_level_hops() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let building_1_id = namespace.urn("Building", 1)?;
    let building_2_id = namespace.urn("Building", 2)?;
    let city_type = namespace.entity_type("City")?;
    let city_1_id = namespace.urn("City", 1)?;
    let city_2_id = namespace.urn("City", 2)?;
    let country_type = namespace.entity_type("Country")?;
    let country_1_id = namespace.urn("Country", 1)?;
    let country_2_id = namespace.urn("Country", 2)?;
    let entities = [
        Entity::builder()
            .id(building_1_id.clone())
            .entity_type(building_type.clone())
            .relationship("locatedAt", Relationship::builder(city_1_id.clone()).object_type(VocabTerm::from(&city_type)))
            .build()?,
        Entity::builder()
            .id(city_1_id.clone())
            .entity_type(city_type.clone())
            .relationship(
                "locatedAt",
                Relationship::builder(country_1_id.clone()).object_type(VocabTerm::from(&country_type)),
            )
            .build()?,
        Entity::builder().id(country_1_id.clone()).entity_type(country_type.clone()).build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .relationship("locatedAt", Relationship::builder(city_2_id.clone()).object_type(VocabTerm::from(&city_type)))
            .build()?,
        Entity::builder()
            .id(city_2_id.clone())
            .entity_type(city_type.clone())
            .relationship(
                "locatedAt",
                Relationship::builder(country_2_id.clone()).object_type(VocabTerm::from(&country_type)),
            )
            .build()?,
        Entity::builder().id(country_2_id.clone()).entity_type(country_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // Two hops reach the Cities and the Countries.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(2.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(6)));
    expect_that!(
        page.items()
            .iter()
            .take(2)
            .filter(|item| item.entity_type == OneOrMany::one(building_type.clone()))
            .count(),
        eq(2)
    );
    for (building_id, city_id, country_id) in [(&building_1_id, &city_1_id, &country_1_id), (&building_2_id, &city_2_id, &country_2_id)] {
        let building = with_id(page.items(), building_id).or_fail()?;
        expect_that!(member_count(building), eq(3));
        expect_that!(
            building,
            has_attribute("locatedAt", relationship().object(city_id.clone()).object_type(VocabTerm::from(&city_type)))
        );
        let city = with_id(page.items(), city_id).or_fail()?;
        expect_that!(member_count(city), eq(3));
        expect_that!(city.entity_type, eq(&OneOrMany::one(city_type.clone())));
        expect_that!(
            city,
            has_attribute(
                "locatedAt",
                relationship().object(country_id.clone()).object_type(VocabTerm::from(&country_type))
            )
        );
        let country = with_id(page.items(), country_id).or_fail()?;
        expect_that!(member_count(country), eq(2));
        expect_that!(country.entity_type, eq(&OneOrMany::one(country_type.clone())));
    }
    // The same array in simplified representation.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_simplified()
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(2.try_into()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(6)));
    expect_that!(
        page_2
            .items()
            .iter()
            .take(2)
            .filter(|item| item.entity_type == OneOrMany::one(building_type.clone()))
            .count(),
        eq(2)
    );
    for (building_id, city_id, country_id) in [(&building_1_id, &city_1_id, &country_1_id), (&building_2_id, &city_2_id, &country_2_id)] {
        let building = with_id(page_2.items(), building_id).or_fail()?;
        expect_that!(simplified_member_count(building), eq(3));
        expect_that!(building, has_value("locatedAt", city_id.to_string()));
        let city = with_id(page_2.items(), city_id).or_fail()?;
        expect_that!(simplified_member_count(city), eq(3));
        expect_that!(city.entity_type, eq(&OneOrMany::one(city_type.clone())));
        expect_that!(city, has_value("locatedAt", country_id.to_string()));
        let country = with_id(page_2.items(), country_id).or_fail()?;
        expect_that!(simplified_member_count(country), eq(2));
        expect_that!(country.entity_type, eq(&OneOrMany::one(country_type.clone())));
    }
    // One hop on the same graph stops at the Cities.
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(4)));
    expect_that!(
        page_3
            .items()
            .iter()
            .take(2)
            .filter(|item| item.entity_type == OneOrMany::one(building_type.clone()))
            .count(),
        eq(2)
    );
    expect_that!(
        page_3
            .items()
            .iter()
            .filter(|item| item.entity_type == OneOrMany::one(city_type.clone()))
            .count(),
        eq(2)
    );
    expect_that!(
        page_3
            .items()
            .iter()
            .filter(|item| item.entity_type == OneOrMany::one(country_type.clone()))
            .count(),
        eq(0)
    );
    // A chain a -> b -> c -> d -> e -> f -> g; the id restriction makes a the only matching entity.
    let chain_ids = ["a", "b", "c", "d", "e", "f", "g"]
        .into_iter()
        .map(|suffix| namespace.urn("Building", suffix))
        .collect::<Result<Vec<EntityId>, _>>()?;
    let chain = chain_ids
        .iter()
        .enumerate()
        .rev()
        .map(|(index, id)| match chain_ids.get(index + 1) {
            Some(next) => Entity::builder()
                .id(id.clone())
                .entity_type(building_type.clone())
                .relationship("linkedTo", next.clone())
                .build(),
            None => Entity::builder().id(id.clone()).entity_type(building_type.clone()).build(),
        })
        .collect::<Result<Vec<_>, _>>()?;
    broker.entities().batch_create(&chain).send().await?.done()?;
    for (level, length) in [(1_u32, 2_usize), (2, 3), (6, 7)] {
        let page_chain = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .query()
            .entity_type(&building_type)
            .ids(vec![chain_ids.first().or_fail()?.clone()])
            .join(Join::Flat)
            .join_level(level.try_into()?)
            .send()
            .await?;
        expect_that!(page_chain.items(), len(eq(length)));
        expect_that!(page_chain.items(), ids_in_order(chain_ids.get(..length).or_fail()?));
    }
    Ok(())
}
