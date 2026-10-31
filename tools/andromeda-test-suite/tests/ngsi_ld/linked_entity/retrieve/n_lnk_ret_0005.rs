//! Join inline level two nests the second hop.

use andromeda_test_suite::{
    attribute_matcher::relationship,
    broker,
    core_context::core_context,
    entity_matcher::{has_attribute, member_count, projected_member_count},
    identity_matcher::has_id,
    linked_entity::linked_entities,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    simplified_matcher::{has_value, has_value_at, simplified_member_count},
    value::object,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{InlineJoin, RepresentationSpelling},
    },
    model::{Entity, NgsiLdMediaType, OneOrMany, Relationship, Value, VocabTerm},
};

/// Pins: Retrieve Entity with join=inline and `joinLevel=2` nests the City under the Building and the Country under
/// the City, normalized and simplified, as JSON and as JSON-LD
/// Clause: 5.7.1.4, 4.5.23.2, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_lnk_ret_0005_join_inline_level_two_nests_the_second_hop() -> Result<()> {
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
        .retrieve(&building_1_id)
        .join(InlineJoin::Inline)
        .join_level(2.try_into()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(member_count(entity), eq(3));
    expect_that!(entity, has_id(&building_1_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(
        entity,
        has_attribute(
            "locatedAt",
            relationship().object(city_paris_id.clone()).object_type(VocabTerm::from(&city_type))
        )
    );
    let cities = linked_entities(entity, &["locatedAt"]);
    expect_that!(cities.first().map(projected_member_count), some(eq(3)));
    expect_that!(cities, nth(0, has_id(&city_paris_id)));
    expect_that!(cities.first().map(|city| &city.entity_type), some(eq(&Some(OneOrMany::one(city_type.clone())))));
    expect_that!(
        cities,
        nth(
            0,
            has_attribute(
                "locatedAt",
                relationship().object(country_france_id.clone()).object_type(VocabTerm::from(&country_type))
            )
        )
    );
    let countries = linked_entities(entity, &["locatedAt", "locatedAt"]);
    expect_that!(countries.first().map(projected_member_count), some(eq(2)));
    expect_that!(countries, nth(0, has_id(&country_france_id)));
    expect_that!(
        countries.first().map(|country| &country.entity_type),
        some(eq(&Some(OneOrMany::one(country_type.clone()))))
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&building_1_id)
        .join(InlineJoin::Inline)
        .join_level(2.try_into()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(simplified_member_count(entity_2), eq(3));
    expect_that!(entity_2.id, eq(&building_1_id));
    expect_that!(entity_2.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(
        entity_2,
        has_value(
            "locatedAt",
            object([
                ("id", Value::from(city_paris_id.to_string())),
                ("type", Value::from(city_type.as_str())),
                (
                    "locatedAt",
                    object([("id", Value::from(country_france_id.to_string())), ("type", Value::from(country_type.as_str()))])
                ),
            ])
        )
    );
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&building_1_id)
        .join(InlineJoin::Inline)
        .join_level(2.try_into()?)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, has_id(&building_1_id));
    expect_that!(entity_3.context, some(eq(&core_context()?)));
    expect_that!(linked_entities(entity_3, &["locatedAt"]), nth(0, has_id(&city_paris_id)));
    expect_that!(linked_entities(entity_3, &["locatedAt", "locatedAt"]), nth(0, has_id(&country_france_id)));
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve_simplified(&building_1_id)
        .join(InlineJoin::Inline)
        .join_level(2.try_into()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity_4 = retrieved_4.body();
    expect_that!(entity_4.id, eq(&building_1_id));
    expect_that!(entity_4.context, some(eq(&core_context()?)));
    expect_that!(entity_4, has_value_at("locatedAt", ["id"], city_paris_id.to_string()));
    expect_that!(entity_4, has_value_at("locatedAt", ["locatedAt", "id"], country_france_id.to_string()));
    Ok(())
}
