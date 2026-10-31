//! Observation space reads back as a geo property.

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

/// Pins: an entity created with an `observationSpace` `GeoProperty` (a Polygon) reads back with `observationSpace`
/// as a `GeoProperty`
/// Clause: 5.6.1, 4.7, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_0009_observation_space_reads_back_as_a_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property(
            "observationSpace",
            Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                Position::try_new(13.35, 52.51)?,
                Position::try_new(13.40, 52.51)?,
                Position::try_new(13.40, 52.56)?,
                Position::try_new(13.35, 52.56)?,
                Position::try_new(13.35, 52.51)?,
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
    expect_that!(entity, has_attribute("observationSpace", geo_property().value_member(["type"], "Polygon")));
    Ok(())
}
