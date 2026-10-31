//! A concise geometry value beside observed at is a geo property.

use andromeda_test_suite::{attribute_matcher::geo_property, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        GeoProperty,
        Lossless,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
};

/// Pins: Create Entity with location written as {"value": <a `GeoJSON` Point>, "`observedAt`": ...} stores a
/// `GeoProperty` whose value is the Point and which keeps `observedAt`
/// Clause: 4.5.2.3, 5.2.7, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9005_a_concise_geometry_value_beside_observed_at_is_a_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type)
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(13.4, 52.5)?)).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.entities().create(&device).form(Lossless::Concise).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "location",
            geo_property().value_member(["type"], "Point").observed_at("2024-01-01T00:00:00Z".parse()?)
        )
    );
    Ok(())
}
