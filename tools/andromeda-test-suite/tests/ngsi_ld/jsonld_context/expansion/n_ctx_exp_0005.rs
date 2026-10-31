//! An ld json create without a body context is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: an application/ld+json Create Entity whose body names no @context is refused as `BadRequestData` and
/// creates nothing
/// Clause: 6.3.5, 5.6.1
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0005_an_ld_json_create_without_a_body_context_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    // The body goes as application/ld+json, which no typed operation sends, so it is sent as written.
    let request = request_to(&broker, Method::POST, "entities")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Building:{namespace}-1\",\n    \"type\": \"Building_{namespace}\",\n    \"name\": {{\n        \"type\": \"Property\",\n        \"value\": \"Eiffel Tower\"\n    }}\n}}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
