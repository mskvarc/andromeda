//! Geojson retrieve with versions 1 0 1 5 and 1 8 is answered 200 without amendment.

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
        operation::{GeoJson, RetrieveEntity},
    },
    model::{
        AttributeName,
        Entity,
        Lossless,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
};

/// Pins: Retrieve Entity as geo+json with Prefer ngsi-ld=1.0, 1.5 and 1.8 answers 200 each time without amendment
/// and without a Preference-Applied naming the version
/// Clause: 6.3.6, 5.7.1.4, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_cmn_prf_2303_geojson_retrieve_with_versions_1_0_1_5_and_1_8_is_answered_200_without_amendment() -> Result<()> {
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
    let location = "location".parse::<AttributeName>()?;
    let operation = RetrieveEntity::builder()
        .id(&building_id)
        .format(GeoJson {
            form: Lossless::Normalized,
            geometry_property: Some(&location),
        })
        .build();
    let geo_json = broker.with_accept(Accept::new(NgsiLdMediaType::GeoJson));
    // The typed Prefer header writes `ngsi-ld` before `body`, so the headers are sent as written.
    for prefer in ["body=json, ngsi-ld=1.0", "body=json, ngsi-ld=1.5", "body=json, ngsi-ld=1.8"] {
        let answer = send_preferring(&geo_json, &operation, prefer).await?;
        expect_that!(answer.status, eq(StatusCode::OK));
        expect_that!(names_a_version(answer.head.headers()), eq(false));
        expect_that!(answer.outcome.body().entity.id, eq(&building_id));
    }
    Ok(())
}
