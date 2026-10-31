//! Geojson with body ld json carries the context in the body.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, GeoJsonBody, Prefer},
    model::{
        AttributeName,
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
};

/// Pins: Retrieve Entity as geo+json with Prefer body=ld+json carries the @context inside the Feature body
/// Clause: 6.3.6, 5.7.1.4, 4.5.16.2
#[gtest]
#[tokio::test]
async fn n_cmn_prf_0002_geojson_with_body_ld_json_carries_the_context_in_the_body() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .with_prefer(Prefer::builder().geo_json_body(GeoJsonBody::LdJson).build()?)
        .entities()
        .retrieve_geo_json(&building_id)
        .geometry_property(&"location".parse::<AttributeName>()?)
        .send()
        .await?;
    let feature = retrieved.body();
    expect_that!(feature.entity.id, eq(&building_id));
    expect_that!(feature.context, some(anything()));
    Ok(())
}
