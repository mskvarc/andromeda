//! Create entity without a content type is 415.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Create Entity with a body and no Content-Type header answers 415, and nothing is created
/// Clause: 6.3.4, 5.6.1
#[gtest]
#[tokio::test]
async fn n_cmn_cnt_0003_create_entity_without_a_content_type_is_415() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    // The body goes without a Content-Type, which no typed operation sends, so it is sent as written.
    let request = request_to(&broker, Method::POST, "entities")?.body(Bytes::from(format!(
        "{{\"id\": \"urn:ngsi-ld:Building:{namespace}-1\", \"type\": \"Building_{namespace}\"}}"
    )));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE));
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
