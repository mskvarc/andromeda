//! Append attributes declaring merge patch json is 415.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Append Entity Attributes (POST) declaring application/merge-patch+json is refused with a bare 415 and
/// appends nothing
/// Clause: 6.3.4
/// Recorded: 6.3.4 allows application/merge-patch+json for PATCH requests only; any other media type than
/// application/json and application/ld+json is 415 without a body
#[gtest]
#[tokio::test]
async fn n_cmn_cnt_2002_append_attributes_declaring_merge_patch_json_is_415() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).property("speed", 1).build()?;
    broker.entities().create(&device).send().await?.done()?;
    // The body goes as application/merge-patch+json, which no typed operation sends, so it is sent as written.
    let request = request_to(&broker, Method::POST, &format!("entities/{device_id}/attrs"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/merge-patch+json"))
        .body(Bytes::from("{ \"gear\": { \"type\": \"Property\", \"value\": 3 } }"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE).without_body());
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("gear"));
    expect_that!(entity, has_attribute("speed", any_attribute().value(1)));
    Ok(())
}
