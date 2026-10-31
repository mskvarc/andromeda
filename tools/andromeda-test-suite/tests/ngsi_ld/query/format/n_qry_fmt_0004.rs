//! A type selection accepting geojson answers 200.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, problem::answered, unchecked_call::exchange};
use googletest::prelude::*;
use http::{Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
};

/// Pins: Query Entities selecting by type alone with Accept application/geo+json is honoured with 200 and a
/// `GeoJSON` `FeatureCollection`, unlike the resources 6.3.4 does not name
/// Clause: 6.3.4, 5.7.2, 4.5.16
#[gtest]
#[tokio::test]
async fn n_qry_fmt_0004_a_type_selection_accepting_geojson_answers_200() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .query_geo_json()
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items().first().map(|feature| &feature.entity), some(has_id(&building_id)));
    // The media type of the answer is a header field the typed page does not keep, so the same read goes out as written.
    let request = exchange(
        &broker,
        Method::GET,
        &format!("entities?type=Building_{namespace}"),
        &[("Accept", "application/geo+json")],
        None,
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let response = answer?;
    expect_that!(
        response.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()),
        some(eq("application/geo+json"))
    );
    Ok(())
}
