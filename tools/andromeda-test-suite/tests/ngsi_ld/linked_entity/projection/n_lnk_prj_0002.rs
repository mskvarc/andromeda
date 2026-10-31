//! Query omit with a nested group drops members of each linked entity.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, geo_property, property, relationship},
    broker,
    entity_lookup::with_id,
    entity_matcher::{has_attribute, lacks_attribute, projected_member_count},
    identity_matcher::has_id,
    linked_entity::linked_entities,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        LanguageMap,
        LanguageProperty,
        NgsiLdMediaType,
        OneOrMany,
        Value,
        geometry::{Position, Shape},
    },
    query::{Join, projection::Projection},
};

/// Pins: Query Entities with a join and an omit term carrying a nested group drops the named members from the
/// Linked Entity and keeps every other member, while the Relationship stays on the matching entity, in inline and
/// in flat form
/// Clause: 4.21, 4.5.23, 5.7.2.4, 5.7.2.5
#[gtest]
#[tokio::test]
async fn n_lnk_prj_0002_query_omit_with_a_nested_group_drops_members_of_each_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let building_1_id = namespace.urn("Building", 1)?;
    let building_2_id = namespace.urn("Building", 2)?;
    let city_type = namespace.entity_type("City")?;
    let city_1_id = namespace.urn("City", 1)?;
    let city_2_id = namespace.urn("City", 2)?;
    let country_france_id = namespace.urn("Country", "france")?;
    let country_italy_id = namespace.urn("Country", "italy")?;
    let depot_type = namespace.entity_type("Depot")?;
    let depot_a_id = namespace.urn("Depot", "a")?;
    let depot_b_id = namespace.urn("Depot", "b")?;
    let entities = [
        Entity::builder()
            .id(building_1_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .relationship("locatedAt", city_1_id.clone())
            .geo_property("location", Shape::Point(Position::try_new(-8.5, 41.2)?))
            .language_property(
                "street",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place"), ("nl", "Grote Markt")])?),
            )
            .build()?,
        Entity::builder()
            .id(city_1_id.clone())
            .entity_type(city_type.clone())
            .property("name", "Paris")
            .property("description", "Paris is the capital and largest city of France")
            .relationship("isInCountry", country_france_id.clone())
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Pisa Tower")
            .relationship("locatedAt", city_2_id.clone())
            .geo_property("location", Shape::Point(Position::try_new(10.4, 43.7)?))
            .language_property(
                "street",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("it", "Piazza dei Miracoli")])?),
            )
            .build()?,
        Entity::builder()
            .id(city_2_id.clone())
            .entity_type(city_type.clone())
            .property("name", "Pisa")
            .property("description", "Pisa is a city in Tuscany")
            .relationship("isInCountry", country_italy_id.clone())
            .build()?,
        Entity::builder()
            .id(depot_b_id.clone())
            .entity_type(depot_type.clone())
            .property("category", "commercial")
            .property("name", "Depot")
            .build()?,
        Entity::builder()
            .id(depot_a_id.clone())
            .entity_type(depot_type.clone())
            .relationship("linkedTo", depot_b_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // inline, omit=name,locatedAt{name}: name leaves each Building and each nested City, nothing else does
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .omit("name,locatedAt{name}".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    let building_1 = with_id(page.items(), &building_1_id).or_fail()?;
    expect_that!(projected_member_count(building_1), eq(5));
    expect_that!(building_1.entity_type, some(eq(&OneOrMany::one(building_type.clone()))));
    expect_that!(building_1, lacks_attribute("name"));
    expect_that!(
        building_1,
        has_attribute(
            "location",
            geo_property()
                .value_member(["coordinates", "0"], Value::Number("-8.5".parse()?))
                .value_member(["coordinates", "1"], Value::Number("41.2".parse()?))
        )
    );
    expect_that!(
        building_1,
        has_attribute("street", any_attribute().language("fr", "Grand Place").language("nl", "Grote Markt"))
    );
    expect_that!(building_1, has_attribute("locatedAt", relationship().object(city_1_id.clone())));
    let cities = linked_entities(building_1, &["locatedAt"]);
    expect_that!(cities.first().map(projected_member_count), some(eq(4)));
    expect_that!(cities, nth(0, has_id(&city_1_id)));
    expect_that!(cities.first().map(|city| &city.entity_type), some(eq(&Some(OneOrMany::one(city_type.clone())))));
    expect_that!(cities, nth(0, lacks_attribute("name")));
    expect_that!(
        cities,
        nth(
            0,
            has_attribute("description", property().value("Paris is the capital and largest city of France"))
        )
    );
    expect_that!(cities, nth(0, has_attribute("isInCountry", relationship().object(country_france_id.clone()))));
    let building_2 = with_id(page.items(), &building_2_id).or_fail()?;
    expect_that!(projected_member_count(building_2), eq(5));
    expect_that!(building_2, lacks_attribute("name"));
    let cities_2 = linked_entities(building_2, &["locatedAt"]);
    expect_that!(cities_2.first().map(projected_member_count), some(eq(4)));
    expect_that!(cities_2, nth(0, has_id(&city_2_id)));
    expect_that!(cities_2, nth(0, lacks_attribute("name")));
    // flat, omit=name,locatedAt{name}: the Buildings first, then each City without name
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .omit("name,locatedAt{name}".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(4)));
    expect_that!(
        page_2
            .items()
            .iter()
            .take(2)
            .filter(|item| item.entity_type == Some(OneOrMany::one(building_type.clone())))
            .count(),
        eq(2)
    );
    let building_1_flat = with_id(page_2.items(), &building_1_id).or_fail()?;
    expect_that!(projected_member_count(building_1_flat), eq(5));
    expect_that!(building_1_flat, lacks_attribute("name"));
    expect_that!(
        building_1_flat,
        has_attribute("location", geo_property().value_member(["coordinates", "0"], Value::Number("-8.5".parse()?)))
    );
    expect_that!(building_1_flat, has_attribute("street", any_attribute().language("fr", "Grand Place")));
    expect_that!(building_1_flat, has_attribute("locatedAt", relationship().object(city_1_id.clone())));
    expect_that!(linked_entities(building_1_flat, &["locatedAt"]), is_empty());
    let building_2_flat = with_id(page_2.items(), &building_2_id).or_fail()?;
    expect_that!(projected_member_count(building_2_flat), eq(5));
    expect_that!(building_2_flat, lacks_attribute("name"));
    expect_that!(building_2_flat, has_attribute("locatedAt", relationship().object(city_2_id.clone())));
    let city_1_flat = with_id(page_2.items(), &city_1_id).or_fail()?;
    expect_that!(projected_member_count(city_1_flat), eq(4));
    expect_that!(city_1_flat.entity_type, some(eq(&OneOrMany::one(city_type.clone()))));
    expect_that!(city_1_flat, lacks_attribute("name"));
    expect_that!(
        city_1_flat,
        has_attribute("description", property().value("Paris is the capital and largest city of France"))
    );
    expect_that!(city_1_flat, has_attribute("isInCountry", relationship().object(country_france_id.clone())));
    let city_2_flat = with_id(page_2.items(), &city_2_id).or_fail()?;
    expect_that!(projected_member_count(city_2_flat), eq(4));
    expect_that!(city_2_flat, lacks_attribute("name"));
    // the nested omit alone: the root keeps its Relationship and every other member, the Linked Entity loses name only
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&depot_type)
        .ids(vec![depot_a_id.clone()])
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .omit("linkedTo{name}".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    let depot_a = page_3.items().first().or_fail()?;
    expect_that!(projected_member_count(depot_a), eq(3));
    expect_that!(depot_a, has_id(&depot_a_id));
    expect_that!(depot_a.entity_type, some(eq(&OneOrMany::one(depot_type.clone()))));
    expect_that!(depot_a, has_attribute("linkedTo", relationship().object(depot_b_id.clone())));
    let depot_b = page_3.items().get(1).or_fail()?;
    expect_that!(projected_member_count(depot_b), eq(3));
    expect_that!(depot_b, has_id(&depot_b_id));
    expect_that!(depot_b.entity_type, some(eq(&OneOrMany::one(depot_type.clone()))));
    expect_that!(depot_b, has_attribute("category", property().value("commercial")));
    expect_that!(depot_b, lacks_attribute("name"));
    Ok(())
}
