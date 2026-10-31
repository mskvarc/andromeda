//! Post query geometry property without a geojson accept is bad request data.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, outcome::Done, problem::problem, unchecked_call::exchange};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{
        AttributeName,
        Entity,
        ErrorType,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
};

/// Pins: Query Entities by POST naming `geometryProperty` in its URL is refused with `BadRequestData` under every
/// Accept other than application/geo+json, with an application/json and an application/ld+json body alike, and is
/// answered under application/geo+json
/// Clause: 5.7.2.4, 6.3.15, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_ent_3002_post_query_geometry_property_without_a_geojson_accept_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id)
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(2.2945, 48.8584)?))
        .build()?;
    broker.entities().batch_create(&[building]).send().await?.done()?;
    // The client cannot write geometryProperty beside a body, so each call goes out as written.
    let plain = format!(r#"{{"type": "Query", "entities": [{{"type": "Building_{namespace}"}}]}}"#);
    let with_q = format!(r#"{{"type": "Query", "q": "location", "entities": [{{"type": "Building_{namespace}"}}]}}"#);
    let linked = format!(
        r#"{{"type": "Query", "entities": [{{"type": "Building_{namespace}"}}], "@context": "{}"}}"#,
        config.core_context()
    );
    let json = [("Content-Type", "application/json"), ("Accept", "application/json")];
    let ld_json = [("Content-Type", "application/ld+json"), ("Accept", "application/ld+json")];
    let calls = [
        ("entityOperations/query?geometryProperty=location", &json, &plain),
        ("entityOperations/query?geometryProperty=location", &ld_json, &linked),
        ("entityOperations/query?geometryProperty=location&options=keyValues", &json, &plain),
        ("entityOperations/query?geometryProperty=location", &json, &with_q),
    ];
    for (target, fields, body) in calls {
        let request = exchange(&broker, Method::POST, target, fields, Some(body.as_str()))?;
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    }
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .query_geo_json()
        .method(QueryMethod::Post)
        .geometry_property(&"location".parse::<AttributeName>()?)
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    Ok(())
}
