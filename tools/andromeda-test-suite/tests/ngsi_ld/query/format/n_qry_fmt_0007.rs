//! Geojson gives an entity without location a null geometry.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_attribute, member_count},
    namespace::Namespace,
    outcome::Done,
    type_matcher::has_type,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Value},
};

/// Pins: Query Entities as geo+json gives an entity without a location a Feature whose geometry is null and whose
/// properties still carry its type and attributes
/// Clause: 4.5.16.1, 4.5.16.3, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_fmt_0007_geojson_gives_an_entity_without_location_a_null_geometry() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Value::Number("22.0".parse()?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .query_geo_json()
        .ids(vec![building_id.clone()])
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let feature = page.items().first().or_fail()?;
    expect_that!(feature.entity.id, eq(&building_id));
    expect_that!(feature.geometry, none());
    expect_that!(member_count(&feature.entity), eq(3));
    expect_that!(&feature.entity, has_type(&building_type));
    expect_that!(&feature.entity, has_attribute("temperature", property().value(Value::Number("22.0".parse()?))));
    Ok(())
}
