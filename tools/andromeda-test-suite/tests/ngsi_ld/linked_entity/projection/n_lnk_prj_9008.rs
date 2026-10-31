//! A nested omit leaves the identity it does not name.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    identity_matcher::has_id,
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
    query::projection::Projection,
};

/// Pins: Retrieve Entity with join=flat and `omit=linkedTo{name}` keeps the Relationship on the root and the Linked
/// Entity id and type without name, and with omit=linkedTo{id,type} removes the Linked Entity id and type
/// Clause: 4.21, 4.5.23, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_lnk_prj_9008_a_nested_omit_leaves_the_identity_it_does_not_name() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "linked-omit")?;
    let building_type = namespace.entity_type("Building")?;
    let city_id = namespace.urn("City", "linked-omit")?;
    let city_type = namespace.entity_type("City")?;
    let country_id = namespace.urn("Country", "linked-omit")?;
    let country_type = namespace.entity_type("Country")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Depot")
        .relationship("linkedTo", city_id.clone())
        .geo_property("location", Shape::Point(Position::try_new(1.5, 2.5)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let city = Entity::builder()
        .id(city_id.clone())
        .entity_type(city_type.clone())
        .property("name", "Paris")
        .relationship("linkedTo", country_id.clone())
        .build()?;
    broker.entities().create(&city).send().await?.done()?;
    let country = Entity::builder()
        .id(country_id.clone())
        .entity_type(country_type.clone())
        .property("name", "France")
        .build()?;
    broker.entities().create(&country).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_id)
        .join_level(1.try_into()?)
        .omit("linkedTo{name}".parse::<Projection>()?)
        .send()
        .await?;
    let entities = retrieved.body().as_slice();
    expect_that!(entities, nth(0, has_attribute("linkedTo", any_attribute())));
    expect_that!(entities, nth(1, has_id(&city_id)));
    expect_that!(
        entities.get(1).map(|linked| &linked.entity_type),
        some(eq(&Some(OneOrMany::one(city_type.clone()))))
    );
    expect_that!(entities, nth(1, lacks_attribute("name")));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_id)
        .join_level(1.try_into()?)
        .omit("linkedTo{id,type}".parse::<Projection>()?)
        .send()
        .await?;
    let entities_2 = retrieved_2.body().as_slice();
    expect_that!(entities_2.get(1).map(|linked| linked.id.is_none()), some(eq(true)));
    expect_that!(entities_2.get(1).map(|linked| linked.entity_type.is_none()), some(eq(true)));
    Ok(())
}
