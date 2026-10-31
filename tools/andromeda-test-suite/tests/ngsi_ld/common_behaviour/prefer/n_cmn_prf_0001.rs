//! Geojson by default carries the context in the body.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        AttributeName,
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
};

/// Pins: Retrieve Entity as geo+json without a Prefer header carries the @context inside the Feature body
/// Clause: 6.3.6, 5.7.1.4, 4.5.16.2
#[gtest]
#[tokio::test]
async fn n_cmn_prf_0001_geojson_by_default_carries_the_context_in_the_body() -> Result<()> {
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
