//! Geojson with body json and a version is answered 200 without amendment.

use andromeda_test_suite::{
    broker,
    core_context::core_context_link,
    namespace::Namespace,
    outcome::Done,
    prefer_text::{names_a_version, send_preferring},
    wire::header_values,
};
use googletest::prelude::*;
use http::{StatusCode, header::LINK};
use ngsi_ld::{
    http::{
        headers::{Accept, ContentType, Link},
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

/// Pins: Retrieve Entity as geo+json with Prefer body=json, ngsi-ld=1.3 answers 200 with the Feature and the
/// @context in a Link header; the version preference does not apply to a `GeoJSON` answer, so no Preference-Applied
/// names it
/// Clause: 6.3.6, 5.7.1.4, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_cmn_prf_0011_geojson_with_body_json_and_a_version_is_answered_200_without_amendment() -> Result<()> {
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
    // The typed Prefer header writes `ngsi-ld` before `body`, so the header is sent as written.
    let answer = send_preferring(&broker.with_accept(Accept::new(NgsiLdMediaType::GeoJson)), &operation, "body=json, ngsi-ld=1.3").await?;
    expect_that!(answer.status, eq(StatusCode::OK));
    expect_that!(
        answer.head.typed::<ContentType>().valid(),
        some(eq(&ContentType::NgsiLd(NgsiLdMediaType::GeoJson)))
    );
    expect_that!(names_a_version(answer.head.headers()), eq(false));
    expect_that!(header_values(answer.head.headers(), &LINK), len(eq(1)));
    expect_that!(answer.head.typed::<Link>().valid(), some(eq(&core_context_link()?)));
    let feature = answer.outcome.body();
    expect_that!(feature.entity.id, eq(&building_id));
    expect_that!(feature.context, none());
    Ok(())
}
