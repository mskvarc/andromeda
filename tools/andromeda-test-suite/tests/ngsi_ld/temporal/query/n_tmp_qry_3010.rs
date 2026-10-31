//! Post query empty entities attrs pick or omit array is bad request data.

use andromeda_test_suite::{broker, config::SuiteConfig, core_context::core_context, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::QueryMethod},
    model::{ErrorType, NgsiLdMediaType},
    query::temporal_query,
};

/// Pins: a Query body posted to Query Temporal Evolution of Entities is refused with `BadRequestData` when its
/// entities, attrs, pick or omit member is the empty array, the same restriction the entity query body carries
/// Clause: 5.2.23, 5.7.4.4, 5.5.4, 6.24.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_qry_3010_post_query_empty_entities_attrs_pick_or_omit_array_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let core_context_body = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let core = config.core_context();
    // control: the same body without an empty array is a valid temporal query
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&namespace.entity_type("Building")?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    // Table 5.2.23-1: "Empty array (0 length) is not allowed" for entities, attrs, omit and pick, whichever operation the Query is for
    // The client writes no empty list of entity selectors, so the document is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "temporal/entityOperations/query")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from("{\n  \"type\": \"Query\",\n  \"entities\": [],\n  \"attrs\": [ \"height\" ],\n  \"temporalQ\": { \"timerel\": \"after\", \"timeAt\": \"2000-01-01T00:00:00Z\" }\n}"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses a member of this document, so it is sent as written.
    let request_2 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "temporal/entityOperations/query")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Building_{namespace}\" }} ],\n  \"attrs\": [],\n  \"temporalQ\": {{ \"timerel\": \"after\", \"timeAt\": \"2000-01-01T00:00:00Z\" }}\n}}")));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses a member of this document, so it is sent as written.
    let request_3 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "temporal/entityOperations/query")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Building_{namespace}\" }} ],\n  \"pick\": [],\n  \"temporalQ\": {{ \"timerel\": \"after\", \"timeAt\": \"2000-01-01T00:00:00Z\" }}\n}}")));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses a member of this document, so it is sent as written.
    let request_4 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "temporal/entityOperations/query")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Building_{namespace}\" }} ],\n  \"omit\": [],\n  \"temporalQ\": {{ \"timerel\": \"after\", \"timeAt\": \"2000-01-01T00:00:00Z\" }}\n}}")));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client writes no empty list of entity selectors, so the document is sent as written.
    let request_5 = request_to(&core_context_body.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "temporal/entityOperations/query")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json")).body(Bytes::from(format!("{{\n  \"type\": \"Query\",\n  \"entities\": [],\n  \"q\": \"height>1\",\n  \"temporalQ\": {{ \"timerel\": \"after\", \"timeAt\": \"2000-01-01T00:00:00Z\" }},\n  \"@context\": \"{core}\"\n}}")));
    let answer_5 = core_context_body
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .send_unchecked(request_5)
        .await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
