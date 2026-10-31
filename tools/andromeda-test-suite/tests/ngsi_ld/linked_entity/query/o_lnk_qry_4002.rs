//! A post query join level that is not an integer is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
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
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::Join,
};

/// Pins: Query Entities by POST with a `joinLevel` member that is not an integer (1.5, the string abc) is refused
/// with `BadRequestData` under join inline and flat, and `joinLevel` 1 is answered
/// Clause: 5.2.23, 6.23.3.1, 4.5.23, 5.5.2, 5.5.4
/// Recorded: clause 5.5.4 makes a JSON-LD payload whose data "is not syntactically correct, according to the
/// @context or the API data type definitions" `BadRequestData`, so a `joinLevel` member that is no positive integer
/// is `BadRequestData` however it is wrong; only the URL parameter, outside clause 5.5.4, is `InvalidRequest` when
/// it is no integer (Table 5.5.2-1)
#[gtest]
#[tokio::test]
async fn o_lnk_qry_4002_a_post_query_join_level_that_is_not_an_integer_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let building_a_id = namespace.urn("Building", "a")?;
    let building_a = Entity::builder().id(building_a_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building_a).send().await?.done()?;
    for (join, join_level) in [("inline", "1.5"), ("flat", "\"abc\"")] {
        // The model writes only a positive integer joinLevel, so the body is sent as written.
        let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
            .append_header(ACCEPT, HeaderValue::from_static("application/json"))
            .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
            .body(Bytes::from(format!(
                "{{\n    \"type\": \"Query\",\n    \"entities\": [\n        {{\n            \"type\": \"Building_{namespace}\"\n        }}\n    ],\n    \"join\": \"{join}\",\n    \"joinLevel\": {join_level}\n}}"
            )));
        let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    }
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    Ok(())
}
