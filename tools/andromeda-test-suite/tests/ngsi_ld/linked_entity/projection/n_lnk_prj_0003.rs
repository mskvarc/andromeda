//! Retrieve pick narrows each linked entity by its nested group.

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
    http::{headers::Accept, operation::InlineJoin},
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::projection::Projection,
};

/// Pins: Retrieve Entity with a join and pick narrows each Linked Entity by the nested group of the Relationship
/// that reaches it, id and type kept only when named, identically inline and flat, down to the second level
/// Clause: 4.21, 4.5.23, 5.7.1.4, 5.7.1.5
#[gtest]
#[tokio::test]
async fn n_lnk_prj_0003_retrieve_pick_narrows_each_linked_entity_by_its_nested_group() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let country_r_id = namespace.urn("Country", "r")?;
    let country_type = namespace.entity_type("Country")?;
    let city_r_id = namespace.urn("City", "r")?;
    let city_type = namespace.entity_type("City")?;
    let building_r_id = namespace.urn("Building", "r")?;
    let building_type = namespace.entity_type("Building")?;
    let country_france_id = namespace.urn("Country", "france")?;
    let city_paris_id = namespace.urn("City", "paris")?;
    let building_e_id = namespace.urn("Building", "e")?;
    let building_pb_id = namespace.urn("Building", "pb")?;
    let building_pa_id = namespace.urn("Building", "pa")?;
    let entities = [
        Entity::builder()
            .id(country_r_id.clone())
            .entity_type(country_type.clone())
            .property("name", "France")
            .build()?,
        Entity::builder()
            .id(city_r_id.clone())
            .entity_type(city_type.clone())
            .property("name", "Paris")
            .relationship("linkedTo", country_r_id.clone())
            .build()?,
        Entity::builder()
            .id(building_r_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Depot")
            .relationship("linkedTo", city_r_id.clone())
            .geo_property("location", Shape::Point(Position::try_new(1.0, 2.0)?))
            .build()?,
        Entity::builder()
            .id(country_france_id.clone())
            .entity_type(country_type.clone())
            .property("name", "France")
            .property("description", "France is an European country")
            .build()?,
        Entity::builder()
            .id(city_paris_id.clone())
            .entity_type(city_type.clone())
            .property("name", "Paris")
            .property("description", "Paris is the capital and largest city of France")
            .relationship("isInCountry", country_france_id.clone())
            .build()?,
        Entity::builder()
            .id(building_e_id.clone())
            .entity_type(building_type.clone())
            .relationship("locatedAt", city_paris_id.clone())
            .build()?,
        Entity::builder()
            .id(building_pb_id.clone())
            .entity_type(building_type.clone())
            .property("category", "commercial")
            .property("name", "Depot")
            .build()?,
        Entity::builder()
            .id(building_pa_id.clone())
            .entity_type(building_type.clone())
            .relationship("linkedTo", building_pb_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // inline, pick=linkedTo{category,id},id: the nested copy keeps id and category, not name or type
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_pa_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .pick("linkedTo{category,id},id".parse::<Projection>()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(projected_member_count(entity), eq(2));
    expect_that!(entity, has_id(&building_pa_id));
    let linked = linked_entities(entity, &["linkedTo"]);
    expect_that!(linked.first().map(projected_member_count), some(eq(2)));
    expect_that!(linked, nth(0, has_id(&building_pb_id)));
    expect_that!(linked, nth(0, has_attribute("category", property().value("commercial"))));
    expect_that!(linked, nth(0, lacks_attribute("name")));
    expect_that!(linked.first().map(|building| building.entity_type.is_none()), some(eq(true)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_r_id)
        .join(InlineJoin::Inline)
        .join_level(1.try_into()?)
        .pick("id,linkedTo{id,name}".parse::<Projection>()?)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(projected_member_count(entity_2), eq(2));
    let linked_2 = linked_entities(entity_2, &["linkedTo"]);
    expect_that!(linked_2.first().map(projected_member_count), some(eq(2)));
    expect_that!(linked_2, nth(0, has_id(&city_r_id)));
    expect_that!(linked_2, nth(0, has_attribute("name", property().value("Paris"))));
    expect_that!(linked_2.first().map(|city| city.entity_type.is_none()), some(eq(true)));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_r_id)
        .join_level(1.try_into()?)
        .pick("id,linkedTo{id,name}".parse::<Projection>()?)
        .send()
        .await?;
    let entities_3 = retrieved_3.body().as_slice();
    expect_that!(entities_3, len(eq(2)));
    expect_that!(entities_3, nth(0, has_id(&building_r_id)));
    expect_that!(entities_3.get(1).map(projected_member_count), some(eq(2)));
    expect_that!(entities_3, nth(1, has_id(&city_r_id)));
    expect_that!(entities_3, nth(1, has_attribute("name", property().value("Paris"))));
    expect_that!(entities_3.get(1).map(|city| city.entity_type.is_none()), some(eq(true)));
    // the group without a keyword leaves the Linked Entity no identity
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_r_id)
        .join_level(1.try_into()?)
        .pick("id,linkedTo{name}".parse::<Projection>()?)
        .send()
        .await?;
    let entities_4 = retrieved_4.body().as_slice();
    expect_that!(entities_4, len(eq(2)));
    expect_that!(entities_4.get(1).map(projected_member_count), some(eq(1)));
    expect_that!(entities_4, nth(1, has_attribute("name", property().value("Paris"))));
    expect_that!(entities_4.get(1).map(|city| city.id.is_none()), some(eq(true)));
    expect_that!(entities_4.get(1).map(|city| city.entity_type.is_none()), some(eq(true)));
    expect_that!(entities_4, nth(1, lacks_attribute("linkedTo")));
    // inline, pick=id,locatedAt{id,name} under joinLevel=2
    let retrieved_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_e_id)
        .join(InlineJoin::Inline)
        .join_level(2.try_into()?)
        .pick("id,locatedAt{id,name}".parse::<Projection>()?)
        .send()
        .await?;
    let entity_5 = retrieved_5.body();
    expect_that!(projected_member_count(entity_5), eq(2));
    expect_that!(entity_5, has_id(&building_e_id));
    expect_that!(entity_5, has_attribute("locatedAt", relationship().object(city_paris_id.clone())));
    let linked_5 = linked_entities(entity_5, &["locatedAt"]);
    expect_that!(linked_5.first().map(projected_member_count), some(eq(2)));
    expect_that!(linked_5, nth(0, has_id(&city_paris_id)));
    expect_that!(linked_5, nth(0, has_attribute("name", property().value("Paris"))));
    // flat, pick=id,locatedAt{id,name} under joinLevel=2: the Country is named by nothing returned, so it is not appended
    let retrieved_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_e_id)
        .join_level(2.try_into()?)
        .pick("id,locatedAt{id,name}".parse::<Projection>()?)
        .send()
        .await?;
    let entities_6 = retrieved_6.body().as_slice();
    expect_that!(entities_6, len(eq(2)));
    expect_that!(entities_6.first().map(projected_member_count), some(eq(2)));
    expect_that!(entities_6, nth(0, has_id(&building_e_id)));
    expect_that!(entities_6, nth(0, has_attribute("locatedAt", relationship().object(city_paris_id.clone()))));
    expect_that!(
        entities_6.first().map(|building| linked_entities(building, &["locatedAt"]).is_empty()),
        some(eq(true))
    );
    expect_that!(entities_6.get(1).map(projected_member_count), some(eq(2)));
    expect_that!(entities_6, nth(1, has_id(&city_paris_id)));
    expect_that!(entities_6, nth(1, has_attribute("name", property().value("Paris"))));
    // inline, the pick down to the second level
    let retrieved_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_e_id)
        .join(InlineJoin::Inline)
        .join_level(2.try_into()?)
        .pick("id,locatedAt{id,name,isInCountry{id,description}}".parse::<Projection>()?)
        .send()
        .await?;
    let entity_7 = retrieved_7.body();
    expect_that!(projected_member_count(entity_7), eq(2));
    let linked_7 = linked_entities(entity_7, &["locatedAt"]);
    expect_that!(linked_7.first().map(projected_member_count), some(eq(3)));
    expect_that!(linked_7, nth(0, has_id(&city_paris_id)));
    expect_that!(linked_7, nth(0, has_attribute("name", property().value("Paris"))));
    expect_that!(linked_7, nth(0, has_attribute("isInCountry", relationship().object(country_france_id.clone()))));
    let countries = linked_entities(entity_7, &["locatedAt", "isInCountry"]);
    expect_that!(countries.first().map(projected_member_count), some(eq(2)));
    expect_that!(countries, nth(0, has_id(&country_france_id)));
    expect_that!(
        countries,
        nth(0, has_attribute("description", property().value("France is an European country")))
    );
    // flat, the pick down to the second level
    let retrieved_8 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_e_id)
        .join_level(2.try_into()?)
        .pick("id,locatedAt{id,name,isInCountry{id,description}}".parse::<Projection>()?)
        .send()
        .await?;
    let entities_8 = retrieved_8.body().as_slice();
    expect_that!(entities_8, len(eq(3)));
    expect_that!(entities_8.first().map(projected_member_count), some(eq(2)));
    expect_that!(entities_8, nth(0, has_id(&building_e_id)));
    let city_paris = with_id(entities_8, &city_paris_id).or_fail()?;
    expect_that!(projected_member_count(city_paris), eq(3));
    expect_that!(city_paris, has_attribute("name", property().value("Paris")));
    expect_that!(city_paris, has_attribute("isInCountry", relationship().object(country_france_id.clone())));
    let country_france = with_id(entities_8, &country_france_id).or_fail()?;
    expect_that!(projected_member_count(country_france), eq(2));
    expect_that!(country_france, has_attribute("description", property().value("France is an European country")));
    Ok(())
}
