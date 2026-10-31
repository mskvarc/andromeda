//! Query omit through a linked entity term leaves the unnamed second hop whole.

use andromeda_test_suite::{
    attribute_matcher::{property, relationship},
    broker,
    entity_lookup::with_id,
    entity_matcher::{has_attribute, lacks_attribute},
    identity_matcher::has_id,
    linked_entity::linked_entities,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{AttributeName, Entity, NgsiLdMediaType, OneOrMany, SimplifiedValue, Value},
    query::{Join, projection::Projection},
};

/// Pins: Query Entities with `joinLevel=2` and an omit term whose group names one Relationship (`locatedAt{name}`)
/// removes name from the Linked Entity that Relationship reaches only: the Entity reached through the next, unnamed
/// Relationship (`isInCountry`) is returned whole, in inline and in flat form, and with the representation of a
/// `keyValues` answer
/// Clause: 4.21, 4.5.23, 5.7.2.4, 5.7.2.5
#[gtest]
#[tokio::test]
async fn n_lnk_prj_5001_query_omit_through_a_linked_entity_term_leaves_the_unnamed_second_hop_whole() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let building_1_id = namespace.urn("Building", 1)?;
    let city_type = namespace.entity_type("City")?;
    let city_1_id = namespace.urn("City", 1)?;
    let country_type = namespace.entity_type("Country")?;
    let country_1_id = namespace.urn("Country", 1)?;
    let entities = [
        Entity::builder()
            .id(building_1_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .relationship("locatedAt", city_1_id.clone())
            .build()?,
        Entity::builder()
            .id(city_1_id.clone())
            .entity_type(city_type.clone())
            .property("name", "Paris")
            .relationship("isInCountry", country_1_id.clone())
            .build()?,
        Entity::builder()
            .id(country_1_id.clone())
            .entity_type(country_type.clone())
            .property("name", "France")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // inline, omit=type,locatedAt{name}
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .join(Join::Inline)
        .join_level(2.try_into()?)
        .omit("type,locatedAt{name}".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_1_id)));
    expect_that!(page.items(), nth(0, has_attribute("name", property().value("Eiffel Tower"))));
    let building = page.items().first().or_fail()?;
    expect_that!(building.entity_type.is_none(), eq(true));
    let cities = linked_entities(building, &["locatedAt"]);
    expect_that!(cities, nth(0, has_id(&city_1_id)));
    expect_that!(cities.first().map(|city| &city.entity_type), some(eq(&Some(OneOrMany::one(city_type.clone())))));
    expect_that!(cities, nth(0, lacks_attribute("name")));
    let countries = linked_entities(building, &["locatedAt", "isInCountry"]);
    expect_that!(countries, nth(0, has_id(&country_1_id)));
    expect_that!(
        countries.first().map(|country| &country.entity_type),
        some(eq(&Some(OneOrMany::one(country_type.clone()))))
    );
    expect_that!(countries, nth(0, has_attribute("name", property().value("France"))));
    // flat, omit=type,locatedAt{name}
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(2.try_into()?)
        .omit("type,locatedAt{name}".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(3)));
    let building_flat = with_id(page_2.items(), &building_1_id).or_fail()?;
    expect_that!(building_flat, has_attribute("name", property().value("Eiffel Tower")));
    expect_that!(building_flat.entity_type.is_none(), eq(true));
    let city_flat = with_id(page_2.items(), &city_1_id).or_fail()?;
    expect_that!(city_flat.entity_type, some(eq(&OneOrMany::one(city_type.clone()))));
    expect_that!(city_flat, has_attribute("isInCountry", relationship().object(country_1_id.clone())));
    expect_that!(city_flat, lacks_attribute("name"));
    let country_flat = with_id(page_2.items(), &country_1_id).or_fail()?;
    expect_that!(country_flat.entity_type, some(eq(&OneOrMany::one(country_type.clone()))));
    expect_that!(country_flat, has_attribute("name", property().value("France")));
    // flat, keyValues
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected_simplified()
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(2.try_into()?)
        .omit("type,locatedAt{name}".parse::<Projection>()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(3)));
    let name = "name".parse::<AttributeName>()?;
    let building_simplified = page_3.items().iter().find(|item| item.id.as_ref() == Some(&building_1_id)).or_fail()?;
    expect_that!(
        building_simplified.attributes.get(&name),
        some(eq(&SimplifiedValue::Value(Value::from("Eiffel Tower"))))
    );
    expect_that!(building_simplified.entity_type.is_none(), eq(true));
    let city_simplified = page_3.items().iter().find(|item| item.id.as_ref() == Some(&city_1_id)).or_fail()?;
    expect_that!(city_simplified.entity_type, some(eq(&OneOrMany::one(city_type.clone()))));
    expect_that!(city_simplified.attributes.get(&name), none());
    let country_simplified = page_3.items().iter().find(|item| item.id.as_ref() == Some(&country_1_id)).or_fail()?;
    expect_that!(country_simplified.entity_type, some(eq(&OneOrMany::one(country_type.clone()))));
    expect_that!(
        country_simplified.attributes.get(&name),
        some(eq(&SimplifiedValue::Value(Value::from("France"))))
    );
    Ok(())
}
