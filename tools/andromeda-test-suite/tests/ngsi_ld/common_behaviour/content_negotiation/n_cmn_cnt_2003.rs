//! Batch delete declaring merge patch json is 415.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::answered, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Batch Entity Delete (POST) declaring application/merge-patch+json is refused with a bare 415 and deletes
/// nothing
/// Clause: 6.3.4, 5.6.10
/// Recorded: 6.3.4 allows application/merge-patch+json for PATCH requests only; any other media type than
/// application/json and application/ld+json is 415 without a body
#[gtest]
#[tokio::test]
async fn n_cmn_cnt_2003_batch_delete_declaring_merge_patch_json_is_415() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).build()?;
    broker.entities().create(&device).send().await?.done()?;
    // The body goes as application/merge-patch+json, which no typed operation sends, so it is sent as written.
    let request = request_to(&broker, Method::POST, "entityOperations/delete")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/merge-patch+json"))
        .body(Bytes::from(format!("[ \"urn:ngsi-ld:Device:{namespace}-1\" ]")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE).without_body());
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    Ok(())
}
