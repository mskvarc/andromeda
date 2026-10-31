//! Post query ordering without order by is bad request data.

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
    query::{
        Locality,
        ordering::{OrderBy, builders::EntityOrderingBuilder},
    },
};

/// Pins: a Query body whose ordering member lacks `orderBy` is refused with `BadRequestData` in every shape the
/// member takes (empty object, collation alone, a distance reference alone), under the local scope that admits
/// ordering, and the same ordering with `orderBy` orders the answer
/// Clause: 5.2.43, 5.2.23, 5.5.4, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_ord_3001_post_query_ordering_without_order_by_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let core = config.core_context();
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("height", 5)
            .geo_property("location", Shape::Point(Position::try_new(2.2945, 48.8584)?))
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("height", 3)
            .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // Table 5.2.43-1: orderBy has cardinality 1, and none of the other members of OrderingParams substitutes for it
    // The model refuses a member of this document, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::POST,
        "entityOperations/query?local=true",
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"))
    .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
    .body(Bytes::from(format!(
        "{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Building_{namespace}\" }} ],\n  \"ordering\": {{}}\n}}"
    )));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses a member of this document, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::POST,
        "entityOperations/query?local=true",
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"))
    .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
    .body(Bytes::from(format!(
        "{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Building_{namespace}\" }} ],\n  \"ordering\": {{ \"collation\": \"und-x-icu\" }}\n}}"
    )));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses a member of this document, so it is sent as written.
    let request_3 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query?local=true")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Building_{namespace}\" }} ],\n  \"ordering\": {{ \"geometry\": \"Point\", \"coordinates\": [2.2945, 48.8584] }}\n}}")));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses a member of this document, so it is sent as written.
    let request_4 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query?local=true")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json")).body(Bytes::from(format!("{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Building_{namespace}\" }} ],\n  \"ordering\": {{ \"collation\": \"und-x-icu\" }},\n  \"@context\": \"{core}\"\n}}")));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // with orderBy the same body orders the entities
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .ordering(EntityOrderingBuilder::from("height".parse::<OrderBy>()?).collation("und-x-icu"))
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&building_2_id)));
    expect_that!(page.items(), nth(1, has_id(&building_id)));
    Ok(())
}
