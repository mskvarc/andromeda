//! Geojson query by post with a version is answered 200 without amendment.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    prefer_text::{names_a_version, send_preferring},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{GeoJson, QueryEntities, QueryMethod},
    },
    model::{
        AttributeName,
        Entity,
        Lossless,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::builders::SelectorTypeBuilder,
};

/// Pins: Query Entities by POST as geo+json with Prefer body=json, ngsi-ld=1.3 answers 200 with the
/// `FeatureCollection` and no Preference-Applied naming the version
/// Clause: 6.3.6, 5.7.2.4, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_cmn_prf_2305_geojson_query_by_post_with_a_version_is_answered_200_without_amendment() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id)
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let location = "location".parse::<AttributeName>()?;
    let selector = SelectorTypeBuilder::from(&building_type).build()?;
    let operation = QueryEntities::builder()
        .entity_type(&selector)
        .method(QueryMethod::Post)
        .format(GeoJson {
            form: Lossless::Normalized,
            geometry_property: Some(&location),
        })
        .build();
    // The typed Prefer header writes `ngsi-ld` before `body`, so the header is sent as written.
    let answer = send_preferring(&broker.with_accept(Accept::new(NgsiLdMediaType::GeoJson)), &operation, "body=json, ngsi-ld=1.3").await?;
    expect_that!(answer.status, eq(StatusCode::OK));
    expect_that!(names_a_version(answer.head.headers()), eq(false));
    expect_that!(answer.outcome.items(), len(eq(1)));
    Ok(())
}
