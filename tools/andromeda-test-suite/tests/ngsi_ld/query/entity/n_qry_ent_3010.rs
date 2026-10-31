//! Post query empty entities array is bad request data beside any other narrowing.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::problem,
    wire::request_to,
};
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
};

/// Pins: a Query body whose entities member is the empty array is refused with `BadRequestData` whatever else
/// narrows the query (an attribute list, a `geoQ`, the local scope, a q) and whichever body media type carries it
/// Clause: 5.2.23, 5.7.2.4, 5.5.4, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_ent_3010_post_query_empty_entities_array_is_bad_request_data_beside_any_other_narrowing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let core = config.core_context();
    let entities = [Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("height", 5)
        .geo_property("location", Shape::Point(Position::try_new(2.2945, 48.8584)?))
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // Table 5.2.23-1, entities: "Empty array (0 length) is not allowed"
    // The client writes no empty list of entity selectors, so the document is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from("{\n  \"type\": \"Query\",\n  \"entities\": [],\n  \"attrs\": [ \"height\" ]\n}"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client writes no empty list of entity selectors, so the document is sent as written.
    let request_2 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from("{\n  \"type\": \"Query\",\n  \"entities\": [],\n  \"geoQ\": { \"geometry\": \"Point\", \"coordinates\": [2.2945, 48.8584], \"georel\": \"near;maxDistance==1000\" }\n}"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client writes no empty list of entity selectors, so the document is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::POST,
        "entityOperations/query?local=true",
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"))
    .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
    .body(Bytes::from("{\n  \"type\": \"Query\",\n  \"entities\": []\n}"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client writes no empty list of entity selectors, so the document is sent as written.
    let request_4 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(format!(
            "{{\n  \"type\": \"Query\",\n  \"entities\": [],\n  \"q\": \"height>1\",\n  \"@context\": \"{core}\"\n}}"
        )));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // a one-element array is the smallest the member admits
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_id)));
    Ok(())
}
