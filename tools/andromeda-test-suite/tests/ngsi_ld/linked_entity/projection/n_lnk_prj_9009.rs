//! A linked entity term addresses only the entity its relationship reaches.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    linked_entity::linked_entities,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    type_matcher::has_type,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::InlineJoin},
    model::{
        Entity,
        NgsiLdMediaType,
        OneOrMany,
        geometry::{Position, Shape},
    },
    query::{Join, projection::Projection},
};

/// Pins: Retrieve Entity and Query Entities with `joinLevel=2` and omit=type,linkedTo{name}, in both join modes,
/// remove the root type and the City name and leave the Country reached through the City whole; omit=name under
/// join=flat removes name from all three
/// Clause: 4.21, 4.5.23, 5.7.1.4, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_prj_9009_a_linked_entity_term_addresses_only_the_entity_its_relationship_reaches() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_addressed_id = namespace.urn("Building", "addressed")?;
    let building_type = namespace.entity_type("Building")?;
    let city_addressed_id = namespace.urn("City", "addressed")?;
    let city_type = namespace.entity_type("City")?;
    let country_addressed_id = namespace.urn("Country", "addressed")?;
    let country_type = namespace.entity_type("Country")?;
    let building_addressed = Entity::builder()
        .id(building_addressed_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Depot")
        .relationship("linkedTo", city_addressed_id.clone())
        .geo_property("location", Shape::Point(Position::try_new(1.5, 2.5)?))
        .build()?;
    broker.entities().create(&building_addressed).send().await?.done()?;
    let city_addressed = Entity::builder()
        .id(city_addressed_id.clone())
        .entity_type(city_type.clone())
        .property("name", "Paris")
        .relationship("linkedTo", country_addressed_id.clone())
        .build()?;
    broker.entities().create(&city_addressed).send().await?.done()?;
    let country_addressed = Entity::builder()
        .id(country_addressed_id.clone())
        .entity_type(country_type.clone())
        .property("name", "France")
        .build()?;
    broker.entities().create(&country_addressed).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_addressed_id)
        .join(InlineJoin::Inline)
        .join_level(2.try_into()?)
        .omit("type,linkedTo{name}".parse::<Projection>()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.entity_type, none());
    expect_that!(linked_entities(entity, &["linkedTo"]), nth(0, lacks_attribute("name")));
    expect_that!(
        linked_entities(entity, &["linkedTo"])
            .first()
            .and_then(|linked| linked.entity_type.as_ref())
            .and_then(OneOrMany::as_one),
        some(eq(&city_type))
    );
    expect_that!(
        linked_entities(entity, &["linkedTo", "linkedTo"]),
        nth(0, has_attribute("name", any_attribute().value("France")))
    );
    expect_that!(
        linked_entities(entity, &["linkedTo", "linkedTo"])
            .first()
            .and_then(|linked| linked.entity_type.as_ref())
            .and_then(OneOrMany::as_one),
        some(eq(&country_type))
    );
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .ids(vec![building_addressed_id.clone()])
        .join(Join::Inline)
        .join_level(2.try_into()?)
        .omit("type,linkedTo{name}".parse::<Projection>()?)
        .send()
        .await?;
    let item = page.items().first().or_fail()?;
    expect_that!(item.entity_type, none());
    expect_that!(linked_entities(item, &["linkedTo"]), nth(0, lacks_attribute("name")));
    expect_that!(
        linked_entities(item, &["linkedTo"])
            .first()
            .and_then(|linked| linked.entity_type.as_ref())
            .and_then(OneOrMany::as_one),
        some(eq(&city_type))
    );
    expect_that!(
        linked_entities(item, &["linkedTo", "linkedTo"]),
        nth(0, has_attribute("name", any_attribute().value("France")))
    );
    expect_that!(
        linked_entities(item, &["linkedTo", "linkedTo"])
            .first()
            .and_then(|linked| linked.entity_type.as_ref())
            .and_then(OneOrMany::as_one),
        some(eq(&country_type))
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_addressed_id)
        .join_level(2.try_into()?)
        .omit("type,linkedTo{name}".parse::<Projection>()?)
        .send()
        .await?;
    let entities = retrieved_2.body();
    expect_that!(entities.as_slice(), len(eq(3)));
    let item_2 = entities.as_slice().first().or_fail()?;
    expect_that!(item_2.entity_type, none());
    expect_that!(entities.as_slice(), nth(1, all!(lacks_attribute("name"), has_type(&city_type))));
    expect_that!(
        entities.as_slice(),
        nth(2, all!(has_attribute("name", any_attribute().value("France")), has_type(&country_type)))
    );
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .ids(vec![building_addressed_id.clone()])
        .join(Join::Flat)
        .join_level(2.try_into()?)
        .omit("type,linkedTo{name}".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(3)));
    let item_3 = page_2.items().first().or_fail()?;
    expect_that!(item_3.entity_type, none());
    expect_that!(page_2.items(), nth(1, all!(lacks_attribute("name"), has_type(&city_type))));
    expect_that!(
        page_2.items(),
        nth(2, all!(has_attribute("name", any_attribute().value("France")), has_type(&country_type)))
    );
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened(&building_addressed_id)
        .join_level(2.try_into()?)
        .omit("name".parse::<Projection>()?)
        .send()
        .await?;
    let entities_2 = retrieved_3.body();
    expect_that!(entities_2.as_slice(), len(eq(3)));
    expect_that!(entities_2.as_slice(), nth(0, lacks_attribute("name")));
    expect_that!(entities_2.as_slice(), nth(1, lacks_attribute("name")));
    expect_that!(entities_2.as_slice(), nth(2, lacks_attribute("name")));
    Ok(())
}
