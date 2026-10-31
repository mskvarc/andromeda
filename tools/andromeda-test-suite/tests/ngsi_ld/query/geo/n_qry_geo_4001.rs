//! A post query near with both min and max distance is bad request data.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{
        Entity,
        ErrorType,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// Pins: Query Entities by POST with a `geoQ` whose georel is near;minDistance==500;maxDistance==10000 is refused
/// with `BadRequestData`, and the same `geoQ` with one modifier is answered
/// Clause: 4.10, 5.2.13, 5.7.2.4, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_geo_4001_a_post_query_near_with_both_min_and_max_distance_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let spot_band_id = namespace.urn("Spot", "band")?;
    let spot_type = namespace.entity_type("Spot")?;
    let spot_band = Entity::builder()
        .id(spot_band_id.clone())
        .entity_type(spot_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(13.433, 52.534_473)?))
        .build()?;
    broker.entities().create(&spot_band).send().await?.done()?;
    // 4.10: nearRel = nearOp andOp distance equal PositiveNumber admits one modifier; 5.7.2.4 makes a syntactically invalid geoquery BadRequestData.
    // The model refuses a member of this document, so it is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"type\": \"Query\",\n    \"entities\": [\n        {{\n            \"type\": \"Spot_{namespace}\"\n        }}\n    ],\n    \"geoQ\": {{\n        \"georel\": \"near;minDistance==500;maxDistance==10000\",\n        \"geometry\": \"Point\",\n        \"coordinates\": [\n            13.38893,\n            52.534473\n        ]\n    }}\n}}")));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .geo_q(geoquery::near_max(10000, Shape::Point(Position::try_new(13.38893, 52.534_473)?)))
        .entity_type(&spot_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&spot_band_id)));
    Ok(())
}
