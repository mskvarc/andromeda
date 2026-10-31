//! Geojson retrieve with only a version is answered 200 without amendment.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, ContentType, Prefer, SpecVersion},
    model::{
        AttributeName,
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
};

/// Pins: Retrieve Entity as geo+json with Prefer ngsi-ld=1.3 alone, with no body preference, answers 200 with the
/// Feature and no Preference-Applied naming the version
/// Clause: 6.3.6, 5.7.1.4, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_cmn_prf_2301_geojson_retrieve_with_only_a_version_is_answered_200_without_amendment() -> Result<()> {
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
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .with_prefer(Prefer::builder().version("1.3".parse::<SpecVersion>()?).build()?)
        .entities()
        .retrieve_geo_json(&building_id)
        .geometry_property(&"location".parse::<AttributeName>()?)
        .send()
        .await?;
    expect_that!(
        headed.head.typed::<ContentType>().valid(),
        some(eq(&ContentType::NgsiLd(NgsiLdMediaType::GeoJson)))
    );
    let retrieved = &headed.outcome;
    expect_that!(retrieved.preference_applied(), none());
    let entity = retrieved.body();
    expect_that!(entity.entity.id, eq(&building_id));
    Ok(())
}
