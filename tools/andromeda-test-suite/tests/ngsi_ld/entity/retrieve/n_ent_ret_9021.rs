//! Attrs pick and omit reach the default geo property.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
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
    query::projection::Projection,
};

/// Pins: Retrieve Entity with attrs=name or pick=id,name answers no location, with omit=location answers name and
/// no location, and with pick=id,location answers location and no name
/// Clause: 4.7, 4.21, 5.2.4, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ent_ret_9021_attrs_pick_and_omit_reach_the_default_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_geo_projection_id = namespace.urn("Building", "geo-projection")?;
    let building_type = namespace.entity_type("Building")?;
    let city_geo_projection_id = namespace.urn("City", "geo-projection")?;
    let building_geo_projection = Entity::builder()
        .id(building_geo_projection_id.clone())
        .entity_type(building_type)
        .property("name", "Depot")
        .relationship("linkedTo", city_geo_projection_id)
        .geo_property("location", Shape::Point(Position::try_new(1.5, 2.5)?))
        .build()?;
    broker.entities().create(&building_geo_projection).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_geo_projection_id)
        .attrs(vec!["name".parse()?])
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("location"));
    expect_that!(entity, has_attribute("name", any_attribute()));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_geo_projection_id)
        .pick("id,name".parse::<Projection>()?)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, lacks_attribute("location"));
    expect_that!(entity_2, has_attribute("name", any_attribute()));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_geo_projection_id)
        .omit("location".parse::<Projection>()?)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3, lacks_attribute("location"));
    expect_that!(entity_3, has_attribute("name", any_attribute()));
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_projected(&building_geo_projection_id)
        .pick("id,location".parse::<Projection>()?)
        .send()
        .await?;
    let entity_4 = retrieved_4.body();
    expect_that!(entity_4, lacks_attribute("name"));
    expect_that!(entity_4, has_attribute("location", any_attribute()));
    Ok(())
}
