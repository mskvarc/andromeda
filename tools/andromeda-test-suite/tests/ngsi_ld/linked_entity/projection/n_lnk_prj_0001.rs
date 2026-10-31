//! Query pick with a nested group reduces each linked entity.

use andromeda_test_suite::{
    attribute_matcher::{property, relationship},
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
        NgsiLdMediaType,
        OneOrMany,
        geometry::{Position, Shape},
    },
    query::{Join, projection::Projection},
};

/// Pins: Query Entities with a join and a pick term carrying a nested group reduces each Linked Entity to the
/// members the group names, id and type kept only when named, while the Relationship stays on the matching entity,
/// in inline and in flat form
/// Clause: 4.21, 4.5.23, 5.7.2.4, 5.7.2.5
#[gtest]
#[tokio::test]
async fn n_lnk_prj_0001_query_pick_with_a_nested_group_reduces_each_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let building_1_id = namespace.urn("Building", 1)?;
    let building_2_id = namespace.urn("Building", 2)?;
    let city_type = namespace.entity_type("City")?;
    let city_1_id = namespace.urn("City", 1)?;
    let city_2_id = namespace.urn("City", 2)?;
    let country_france_id = namespace.urn("Country", "france")?;
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
            .build()?,
        Entity::builder()
            .id(city_2_id.clone())
            .entity_type(city_type.clone())
            .property("name", "Pisa")
            .property("description", "Pisa is a city in Tuscany")
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
    // inline, pick=id,locatedAt{id,name}: each Building keeps id and locatedAt, each nested City id and name
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .pick("id,locatedAt{id,name}".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    let building_1 = with_id(page.items(), &building_1_id).or_fail()?;
    expect_that!(projected_member_count(building_1), eq(2));
    expect_that!(building_1, has_attribute("locatedAt", relationship().object(city_1_id.clone())));
    let cities = linked_entities(building_1, &["locatedAt"]);
    expect_that!(cities.first().map(projected_member_count), some(eq(2)));
    expect_that!(cities, nth(0, has_id(&city_1_id)));
    expect_that!(cities, nth(0, has_attribute("name", property().value("Paris"))));
    let building_2 = with_id(page.items(), &building_2_id).or_fail()?;
    expect_that!(projected_member_count(building_2), eq(2));
    expect_that!(building_2, has_attribute("locatedAt", relationship().object(city_2_id.clone())));
    let cities_2 = linked_entities(building_2, &["locatedAt"]);
    expect_that!(cities_2.first().map(projected_member_count), some(eq(2)));
    expect_that!(cities_2, nth(0, has_id(&city_2_id)));
    expect_that!(cities_2, nth(0, has_attribute("name", property().value("Pisa"))));
    // flat, pick=id,locatedAt{id,name}: the Buildings first, then each City with id and name only
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .pick("id,locatedAt{id,name}".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(4)));
    let building_prefix = namespace.urn_text("Building", "");
    expect_that!(
        page_2
            .items()
            .iter()
            .take(2)
            .filter(|item| item.id.as_ref().is_some_and(|id| id.to_string().starts_with(&building_prefix)))
            .count(),
        eq(2)
    );
    let building_1_flat = with_id(page_2.items(), &building_1_id).or_fail()?;
    expect_that!(projected_member_count(building_1_flat), eq(2));
    expect_that!(building_1_flat, has_attribute("locatedAt", relationship().object(city_1_id.clone())));
    expect_that!(linked_entities(building_1_flat, &["locatedAt"]), is_empty());
    let building_2_flat = with_id(page_2.items(), &building_2_id).or_fail()?;
    expect_that!(projected_member_count(building_2_flat), eq(2));
    expect_that!(building_2_flat, has_attribute("locatedAt", relationship().object(city_2_id.clone())));
    let city_1_flat = with_id(page_2.items(), &city_1_id).or_fail()?;
    expect_that!(projected_member_count(city_1_flat), eq(2));
    expect_that!(city_1_flat, has_attribute("name", property().value("Paris")));
    let city_2_flat = with_id(page_2.items(), &city_2_id).or_fail()?;
    expect_that!(projected_member_count(city_2_flat), eq(2));
    expect_that!(city_2_flat, has_attribute("name", property().value("Pisa")));
    // inline, pick=linkedTo{category,id},id,type: the nested copy keeps id and category, not type
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&depot_type)
        .ids(vec![depot_a_id.clone()])
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .pick("linkedTo{category,id},id,type".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    let depot_a = page_3.items().first().or_fail()?;
    expect_that!(projected_member_count(depot_a), eq(3));
    expect_that!(depot_a, has_id(&depot_a_id));
    expect_that!(depot_a.entity_type, some(eq(&OneOrMany::one(depot_type.clone()))));
    expect_that!(depot_a, has_attribute("linkedTo", relationship().object(depot_b_id.clone())));
    let depots = linked_entities(depot_a, &["linkedTo"]);
    expect_that!(depots.first().map(projected_member_count), some(eq(2)));
    expect_that!(depots, nth(0, has_id(&depot_b_id)));
    expect_that!(depots, nth(0, has_attribute("category", property().value("commercial"))));
    expect_that!(depots, nth(0, lacks_attribute("name")));
    expect_that!(depots.first().map(|depot| depot.entity_type.is_none()), some(eq(true)));
    // flat, pick=linkedTo{category,id,type},id,type: the Linked Entity keeps id, type and category
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&depot_type)
        .ids(vec![depot_a_id.clone()])
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .pick("linkedTo{category,id,type},id,type".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(2)));
    let depot_a_flat = page_4.items().first().or_fail()?;
    expect_that!(projected_member_count(depot_a_flat), eq(3));
    expect_that!(depot_a_flat, has_id(&depot_a_id));
    expect_that!(depot_a_flat.entity_type, some(eq(&OneOrMany::one(depot_type.clone()))));
    expect_that!(depot_a_flat, has_attribute("linkedTo", relationship().object(depot_b_id.clone())));
    let depot_b_flat = page_4.items().get(1).or_fail()?;
    expect_that!(projected_member_count(depot_b_flat), eq(3));
    expect_that!(depot_b_flat, has_id(&depot_b_id));
    expect_that!(depot_b_flat.entity_type, some(eq(&OneOrMany::one(depot_type.clone()))));
    expect_that!(depot_b_flat, has_attribute("category", property().value("commercial")));
    expect_that!(depot_b_flat, lacks_attribute("name"));
    // flat, pick=linkedTo{category},id: the Linked Entity keeps neither id nor type
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&depot_type)
        .ids(vec![depot_a_id.clone()])
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .pick("linkedTo{category},id".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(2)));
    let depot_a_bare = page_5.items().first().or_fail()?;
    expect_that!(projected_member_count(depot_a_bare), eq(2));
    expect_that!(depot_a_bare, has_id(&depot_a_id));
    expect_that!(depot_a_bare.entity_type.is_none(), eq(true));
    expect_that!(depot_a_bare, has_attribute("linkedTo", relationship().object(depot_b_id.clone())));
    let depot_b_bare = page_5.items().get(1).or_fail()?;
    expect_that!(projected_member_count(depot_b_bare), eq(1));
    expect_that!(depot_b_bare, has_attribute("category", property().value("commercial")));
    expect_that!(depot_b_bare.id.is_none(), eq(true));
    expect_that!(depot_b_bare.entity_type.is_none(), eq(true));
    expect_that!(depot_b_bare, lacks_attribute("name"));
    Ok(())
}
