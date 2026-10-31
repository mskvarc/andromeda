//! The query binding narrows linked entities at two levels.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
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
        geometry::{Position, Shape},
    },
    query::{Join, projection::Projection},
};

/// Pins: Query Entities with `joinLevel=2` and pick=id,linkedTo{id,linkedTo{name}} answers the City with id and no
/// name and the Country with name and no id, nested by join=inline and appended by join=flat
/// Clause: 4.21, 4.5.23, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_prj_9012_the_query_binding_narrows_linked_entities_at_two_levels() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_query_linked_id = namespace.urn("Building", "query-linked")?;
    let building_type = namespace.entity_type("Building")?;
    let city_query_linked_id = namespace.urn("City", "query-linked")?;
    let city_type = namespace.entity_type("City")?;
    let country_query_linked_id = namespace.urn("Country", "query-linked")?;
    let country_type = namespace.entity_type("Country")?;
    let building_query_linked = Entity::builder()
        .id(building_query_linked_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Depot")
        .relationship("linkedTo", city_query_linked_id.clone())
        .geo_property("location", Shape::Point(Position::try_new(1.5, 2.5)?))
        .build()?;
    broker.entities().create(&building_query_linked).send().await?.done()?;
    let city_query_linked = Entity::builder()
        .id(city_query_linked_id.clone())
        .entity_type(city_type)
        .property("name", "Paris")
        .relationship("linkedTo", country_query_linked_id.clone())
        .build()?;
    broker.entities().create(&city_query_linked).send().await?.done()?;
    let country_query_linked = Entity::builder()
        .id(country_query_linked_id.clone())
        .entity_type(country_type)
        .property("name", "France")
        .build()?;
    broker.entities().create(&country_query_linked).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .ids(vec![building_query_linked_id.clone()])
        .join_level(2.try_into()?)
        .pick("id,linkedTo{id,linkedTo{name}}".parse::<Projection>()?)
        .join(Join::Inline)
        .send()
        .await?;
    let item = page.items().first().or_fail()?;
    expect_that!(linked_entities(item, &["linkedTo"]), nth(0, has_id(&city_query_linked_id)));
    expect_that!(linked_entities(item, &["linkedTo"]), nth(0, lacks_attribute("name")));
    expect_that!(
        linked_entities(item, &["linkedTo", "linkedTo"]).first().and_then(|linked| linked.id.as_ref()),
        none()
    );
    expect_that!(linked_entities(item, &["linkedTo", "linkedTo"]), nth(0, has_attribute("name", any_attribute())));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .ids(vec![building_query_linked_id.clone()])
        .join_level(2.try_into()?)
        .pick("id,linkedTo{id,linkedTo{name}}".parse::<Projection>()?)
        .join(Join::Flat)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(3)));
    let item_2 = page_2.items().get(2).or_fail()?;
    expect_that!(item_2.id, none());
    expect_that!(page_2.items(), nth(1, all!(has_id(&city_query_linked_id), lacks_attribute("name"))));
    expect_that!(page_2.items(), nth(2, has_attribute("name", any_attribute())));
    Ok(())
}
