//! A temporal near with both min and max distance is bad request data by get and by post.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use ngsi_ld::{
    http::headers::Accept,
    model::{
        ErrorType,
        GeoProperty,
        NgsiLdMediaType,
        evolution::TemporalEntity,
        geometry::{Position, Shape},
    },
    query::{geoquery, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities by GET and by POST refuses with `BadRequestData` a near georel that
/// carries both `minDistance` and `maxDistance`, and answers one with a single modifier
/// Clause: 4.10, 5.2.13, 5.7.4.4, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_4008_a_temporal_near_with_both_min_and_max_distance_is_bad_request_data_by_get_and_by_post() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let spot_band_id = namespace.urn("Spot", "band")?;
    let spot_type = namespace.entity_type("Spot")?;
    let spot_band_evolution = TemporalEntity::builder()
        .id(spot_band_id.clone())
        .entity_type(spot_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(13.433, 52.534_473)?)).observed_at("2020-08-01T12:03:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&spot_band_evolution).send().await?.created()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("temporal/entities?type=Spot_{namespace}&georel=near%3BminDistance%3D%3D500%3BmaxDistance%3D%3D10000&geometry=Point&coordinates=%5B13.38893%2C52.534473%5D&timerel=after&timeAt=2020-07-01T00%3A00%3A00Z"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses a member of this document, so it is sent as written.
    let request_2 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "temporal/entityOperations/query")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"type\": \"Query\",\n    \"entities\": [\n        {{\n            \"type\": \"Spot_{namespace}\"\n        }}\n    ],\n    \"geoQ\": {{\n        \"georel\": \"near;minDistance==500;maxDistance==10000\",\n        \"geometry\": \"Point\",\n        \"coordinates\": [\n            13.38893,\n            52.534473\n        ]\n    }},\n    \"temporalQ\": {{\n        \"timerel\": \"after\",\n        \"timeAt\": \"2020-07-01T00:00:00Z\"\n    }}\n}}")));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-07-01T00:00:00Z".parse()?))
        .geo_q(geoquery::near_max(10000, Shape::Point(Position::try_new(13.38893, 52.534_473)?)))
        .entity_type(&spot_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&spot_band_id)));
    Ok(())
}
