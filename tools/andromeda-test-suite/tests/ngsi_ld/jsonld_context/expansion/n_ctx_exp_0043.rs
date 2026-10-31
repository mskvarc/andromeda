//! Two context link lines are refused on every verb.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::SuiteConfig,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Retrieve Entity, Delete Entity, Create Entity and Update Attributes carrying two Link lines each naming a
/// @context are refused as `BadRequestData`, and nothing is written, changed or deleted
/// Clause: 6.3.5, JSON-LD 1.1 s6.1
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0043_two_context_link_lines_are_refused_on_every_verb() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let core = config.core_context();
    let vehicle_refused_id = namespace.urn("Vehicle", "refused")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // The client writes one Link field, so the request naming several is sent as written.
    let request = request_to(&broker.with_accept(Accept::any()), Method::GET, &format!("entities/{vehicle_id}"))?.append_header(HeaderName::from_static("link"), HeaderValue::try_from(format!("<{core}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""))?).append_header(HeaderName::from_static("link"), HeaderValue::from_static("<https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""));
    let answer = broker.with_accept(Accept::any()).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client writes one Link field, so the request naming several is sent as written.
    let request_2 = request_to(&broker, Method::DELETE, &format!("entities/{vehicle_id}"))?.append_header(HeaderName::from_static("link"), HeaderValue::try_from(format!("<{core}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""))?).append_header(HeaderName::from_static("link"), HeaderValue::from_static("<https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client writes one Link field, so the request naming several is sent as written.
    let request_3 = request_to(&broker, Method::POST, "entities")?.append_header(HeaderName::from_static("link"), HeaderValue::try_from(format!("<{core}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""))?).append_header(HeaderName::from_static("link"), HeaderValue::from_static("<https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Vehicle:{namespace}-refused\",\n    \"type\": \"Vehicle_{namespace}\"\n}}")));
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client writes one Link field, so the request naming several is sent as written.
    let request_4 = request_to(&broker, Method::PATCH, &format!("entities/{vehicle_id}/attrs"))?.append_header(HeaderName::from_static("link"), HeaderValue::try_from(format!("<{core}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""))?).append_header(HeaderName::from_static("link"), HeaderValue::from_static("<https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from("{\n    \"speed\": {\n        \"type\": \"Property\",\n        \"value\": 2\n    }\n}"));
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(1)));
    let answer_5 = broker
        .with_responses()
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_refused_id)
        .send()
        .await;
    expect_that!(&answer_5, answered(StatusCode::NOT_FOUND));
    Ok(())
}
