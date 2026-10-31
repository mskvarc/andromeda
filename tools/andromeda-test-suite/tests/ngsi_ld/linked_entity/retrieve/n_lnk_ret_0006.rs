//! Join flat level two appends the second hop.

use andromeda_test_suite::{
    attribute_matcher::relationship,
    broker,
    entity_lookup::with_id,
    entity_matcher::{has_attribute, member_count},
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    simplified_matcher::{has_value, simplified_member_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Entity, NgsiLdMediaType, OneOrMany, Relationship, VocabTerm},
};

/// Pins: Retrieve Entity with join=flat and `joinLevel=2` answers the Building, its City and the City's Country,
/// normalized and simplified, as JSON and as JSON-LD
/// Clause: 5.7.1.4, 4.5.23.3, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_lnk_ret_0006_join_flat_level_two_appends_the_second_hop() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let country_type = namespace.entity_type("Country")?;
    let country_france_id = namespace.urn("Country", "france")?;
    let city_type = namespace.entity_type("City")?;
    let city_paris_id = namespace.urn("City", "paris")?;
    let building_type = namespace.entity_type("Building")?;
    let building_1_id = namespace.urn("Building", 1)?;
    let entities = [
        Entity::builder().id(country_france_id.clone()).entity_type(country_type.clone()).build()?,
        Entity::builder()
            .id(city_paris_id.clone())
            .entity_type(city_type.clone())
            .relationship(
                "locatedAt",
                Relationship::builder(country_france_id.clone()).object_type(VocabTerm::from(&country_type)),
            )
            .build()?,
        Entity::builder()
            .id(building_1_id.clone())
            .entity_type(building_type.clone())
            .relationship(
                "locatedAt",
                Relationship::builder(city_paris_id.clone()).object_type(VocabTerm::from(&city_type)),
            )
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&building_1_id)
        .join_level(2.try_into()?)
        .send()
        .await?;
    let entities_1 = retrieved.body().as_slice();
    expect_that!(entities_1, len(eq(3)));
    expect_that!(entities_1.first().map(member_count), some(eq(3)));
    expect_that!(entities_1, nth(0, has_id(&building_1_id)));
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
    let city = with_id(entities_1, &city_paris_id).or_fail()?;
    expect_that!(member_count(city), eq(3));
    expect_that!(city.entity_type, eq(&OneOrMany::one(city_type.clone())));
    expect_that!(
        city,
        has_attribute(
            "locatedAt",
            relationship().object(country_france_id.clone()).object_type(VocabTerm::from(&country_type))
        )
    );
    let country = with_id(entities_1, &country_france_id).or_fail()?;
    expect_that!(member_count(country), eq(2));
    expect_that!(country.entity_type, eq(&OneOrMany::one(country_type.clone())));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_simplified(&building_1_id)
        .join_level(2.try_into()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entities_2 = retrieved_2.body().as_slice();
    expect_that!(entities_2, len(eq(3)));
    expect_that!(entities_2.first().map(simplified_member_count), some(eq(3)));
    expect_that!(entities_2.first().map(|building| &building.id), some(eq(&building_1_id)));
    expect_that!(
        entities_2.first().map(|building| &building.entity_type),
        some(eq(&OneOrMany::one(building_type.clone())))
    );
    expect_that!(entities_2.first(), some(has_value("locatedAt", city_paris_id.to_string())));
    let city_2 = with_id(entities_2, &city_paris_id).or_fail()?;
    expect_that!(simplified_member_count(city_2), eq(3));
    expect_that!(city_2.entity_type, eq(&OneOrMany::one(city_type.clone())));
    expect_that!(city_2, has_value("locatedAt", country_france_id.to_string()));
    let country_2 = with_id(entities_2, &country_france_id).or_fail()?;
    expect_that!(simplified_member_count(country_2), eq(2));
    expect_that!(country_2.entity_type, eq(&OneOrMany::one(country_type.clone())));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve_flattened(&building_1_id)
        .join_level(2.try_into()?)
        .send()
        .await?;
    let entities_3 = retrieved_3.body().as_slice();
    expect_that!(entities_3, len(eq(3)));
    expect_that!(entities_3, nth(0, has_id(&building_1_id)));
    expect_that!(
        with_id(entities_3, &city_paris_id).map(|found| &found.entity_type),
        some(eq(&OneOrMany::one(city_type.clone())))
    );
    expect_that!(
        with_id(entities_3, &country_france_id).map(|found| &found.entity_type),
        some(eq(&OneOrMany::one(country_type.clone())))
    );
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve_flattened_simplified(&building_1_id)
        .join_level(2.try_into()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entities_4 = retrieved_4.body().as_slice();
    expect_that!(entities_4, len(eq(3)));
    expect_that!(entities_4.first().map(|building| &building.id), some(eq(&building_1_id)));
    expect_that!(entities_4.first(), some(has_value("locatedAt", city_paris_id.to_string())));
    expect_that!(with_id(entities_4, &city_paris_id), some(has_value("locatedAt", country_france_id.to_string())));
    expect_that!(
        with_id(entities_4, &country_france_id).map(|found| &found.entity_type),
        some(eq(&OneOrMany::one(country_type.clone())))
    );
    Ok(())
}
