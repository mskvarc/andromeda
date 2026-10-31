//! Merge entity with an unknown parameter is invalid request.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Merge Entity with a URL parameter the operation does not define answers `InvalidRequest` and merges
/// nothing
/// Clause: 6.3.20, 5.6.17
/// Recorded: 6.3.20 says an incompatible parameter "should" give `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_cmn_err_2003_merge_entity_with_an_unknown_parameter_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).property("speed", 1).build()?;
    broker.entities().create(&device).send().await?.done()?;
    // The body goes as application/merge-patch+json, which no typed operation sends, so it is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("entities/{device_id}?invalidParam=invalidValue"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/merge-patch+json"))
        .body(Bytes::from("{ \"speed\": { \"type\": \"Property\", \"value\": 2 } }"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(1)));
    Ok(())
}
