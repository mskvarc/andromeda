//! Geojson of an entity without a geo property has a null geometry.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Value},
};

/// Pins: Retrieve Entity as geo+json gives an entity without any `GeoProperty` a Feature whose geometry is null
/// Clause: 4.5.16.1, 4.5.16.2, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ent_ret_0025_geojson_of_an_entity_without_a_geo_property_has_a_null_geometry() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("temperature", Value::Number("20.0".parse()?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json(&building_id)
        .send()
        .await?;
    let feature = retrieved.body();
    expect_that!(feature.entity.id, eq(&building_id));
    expect_that!(feature.geometry, none());
    Ok(())
}
