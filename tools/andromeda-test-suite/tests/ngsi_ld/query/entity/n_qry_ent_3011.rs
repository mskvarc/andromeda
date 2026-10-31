//! Post query empty attrs pick or omit array is bad request data beside any other narrowing.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    entity_matcher::lacks_attribute,
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
    query::{geoquery, projection::Projection, q},
};

/// Pins: a Query body whose attrs, pick or omit member is the empty array is refused with `BadRequestData` even
/// when entities, q and `geoQ` narrow the query, so the empty array alone is what is refused
/// Clause: 5.2.23, 5.7.2.4, 5.5.4, 4.21, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_ent_3011_post_query_empty_attrs_pick_or_omit_array_is_bad_request_data_beside_any_other_narrowing() -> Result<()> {
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
    // Table 5.2.23-1: attrs, pick and omit all state "Empty array (0 length) is not allowed"; the single-element array beside each is the control
    // The model refuses a member of this document, so it is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Building_{namespace}\" }} ],\n  \"pick\": []\n}}"
        )));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .pick("id".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_id)));
    // The model refuses a member of this document, so it is sent as written.
    let request_2 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n  \"type\": \"Query\",\n  \"q\": \"height>1\",\n  \"entities\": [ {{ \"type\": \"Building_{namespace}\" }} ],\n  \"omit\": []\n}}"
        )));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .q(q::attr("height").gt(1))
        .entity_type(&building_type)
        .omit("location".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, all!(has_id(&building_id), lacks_attribute("location"))));
    // The model refuses a member of this document, so it is sent as written.
    let request_3 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n  \"type\": \"Query\",\n  \"geoQ\": {{ \"geometry\": \"Point\", \"coordinates\": [2.2945, 48.8584], \"georel\": \"near;maxDistance==1000\" }},\n  \"entities\": [ {{ \"type\": \"Building_{namespace}\" }} ],\n  \"attrs\": []\n}}")));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .geo_q(geoquery::near_max(1000, Shape::Point(Position::try_new(2.2945, 48.8584)?)))
        .entity_type(&building_type)
        .attrs(vec!["height".parse()?])
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&building_id)));
    // the refusal holds under an application/ld+json body too
    // The model refuses a member of this document, so it is sent as written.
    let request_4 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(format!(
            "{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Building_{namespace}\" }} ],\n  \"pick\": [],\n  \"@context\": \"{core}\"\n}}"
        )));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses a member of this document, so it is sent as written.
    let request_5 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(format!(
            "{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Building_{namespace}\" }} ],\n  \"omit\": [],\n  \"@context\": \"{core}\"\n}}"
        )));
    let answer_5 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
