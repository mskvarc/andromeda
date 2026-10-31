//! A three dimensional point reads back concise with its altitude.

use andromeda_test_suite::{attribute_matcher::geo_property, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        Lossless,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
};

/// Pins: a three-dimensional Point location reads back in the concise representation as the bare geometry with all
/// three ordinates, because a `GeoProperty` without sub-attributes is its geometry
/// Clause: 4.7.3, 4.5.2.3, 5.7.1
#[gtest]
#[tokio::test]
async fn n_qry_geo_0055_a_three_dimensional_point_reads_back_concise_with_its_altitude() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property("location", Shape::Point(Position::try_with_altitude(13.4, 52.52, 300.0)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .form(Lossless::Concise)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_attribute(
            "location",
            geo_property().geometry(Shape::Point(Position::try_with_altitude(13.4, 52.52, 300.0)?))
        )
    );
    Ok(())
}
