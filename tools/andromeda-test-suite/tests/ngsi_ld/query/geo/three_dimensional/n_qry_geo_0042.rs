//! A geo property the entity does not convey matches nothing for a three dimensional reference.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// Pins: with a three-ordinate reference, a geoproperty naming a plain Property or an attribute the entity lacks
/// matches nothing and is no error, while the location of the same entity matches
/// Clause: 4.10, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_geo_0042_a_geo_property_the_entity_does_not_convey_matches_nothing_for_a_three_dimensional_reference() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(13.4, 52.52)?))
        .property("category", "office")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    // category is a Property, not a GeoProperty.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(100_000, Shape::Point(Position::try_with_altitude(13.4, 52.52, 0.0)?)).geoproperty("category"))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    // absentSpace is not held at all.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(100_000, Shape::Point(Position::try_with_altitude(13.4, 52.52, 0.0)?)).geoproperty("absentSpace"))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    // The control: location is held and is at the reference.
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(100_000, Shape::Point(Position::try_with_altitude(13.4, 52.52, 0.0)?)))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), contains(has_id(&building_id)));
    Ok(())
}
