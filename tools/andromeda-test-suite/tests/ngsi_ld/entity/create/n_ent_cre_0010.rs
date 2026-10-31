//! Operation space reads back as a polygon geo property.

use andromeda_test_suite::{attribute_matcher::geo_property, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
};

/// Pins: an entity created with an `operationSpace` `GeoProperty` (a Polygon) reads back with `operationSpace` as a
/// `GeoProperty` holding a Polygon
/// Clause: 5.6.1, 4.7, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_0010_operation_space_reads_back_as_a_polygon_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property(
            "operationSpace",
            Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                Position::try_new(13.30, 52.50)?,
                Position::try_new(13.45, 52.50)?,
                Position::try_new(13.45, 52.58)?,
                Position::try_new(13.30, 52.58)?,
                Position::try_new(13.30, 52.50)?,
            ])?])?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(
        entity,
        has_attribute(
            "operationSpace",
            geo_property().value_member(["type"], "Polygon").length_at(["coordinates", "0"], 5)
        )
    );
    Ok(())
}
