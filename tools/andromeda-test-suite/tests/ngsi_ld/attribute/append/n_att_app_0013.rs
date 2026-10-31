//! Observation space reads back as a geo property.

use andromeda_test_suite::{attribute_matcher::geo_property, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        EntityFragment,
        NgsiLdMediaType,
        Value,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
};

/// Pins: Append Attributes with an `observationSpace` `GeoProperty` answers 204 and the Entity holds it as a
/// `GeoProperty` with its Polygon
/// Clause: 5.6.3.4, 4.7
#[gtest]
#[tokio::test]
async fn n_att_app_0013_observation_space_reads_back_as_a_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type).build()?;
    broker.entities().create(&building).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .geo_property(
            "observationSpace",
            Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                Position::try_new(13.35, 52.51)?,
                Position::try_new(13.4, 52.51)?,
                Position::try_new(13.4, 52.56)?,
                Position::try_new(13.35, 52.56)?,
                Position::try_new(13.35, 52.51)?,
            ])?])?),
        )
        .build()?;
    broker.entities().append_attributes(&building_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "observationSpace",
            geo_property()
                .value_member(["type"], "Polygon")
                .value_member(["coordinates", "0", "1", "0"], Value::Number("13.4".parse()?))
        )
    );
    Ok(())
}
