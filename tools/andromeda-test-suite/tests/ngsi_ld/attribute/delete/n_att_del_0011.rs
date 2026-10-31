//! Observation space is removed.

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
        AttributeName,
        Entity,
        NgsiLdMediaType,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
};

/// Pins: Delete Attribute addressed to the `observationSpace` `GeoProperty` answers 204 and the Entity no longer
/// has it, keeping its other Attributes
/// Clause: 5.6.5.4, 6.7.3.2
#[gtest]
#[tokio::test]
async fn n_att_del_0011_observation_space_is_removed() -> Result<()> {
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
                Position::try_new(13.4, 52.51)?,
                Position::try_new(13.4, 52.56)?,
                Position::try_new(13.35, 52.56)?,
                Position::try_new(13.35, 52.51)?,
            ])?])?),
        )
        .property("name", "Eiffel Tower")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    broker
        .entities()
        .delete_attribute(&building_id, &"observationSpace".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity, lacks_attribute("observationSpace"));
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    Ok(())
}
