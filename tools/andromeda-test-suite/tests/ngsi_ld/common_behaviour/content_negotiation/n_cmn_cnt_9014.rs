//! Batch delete declaring text plain or no content type is 415.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::answered, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Batch Entity Delete (POST) declaring text/plain, or no Content-Type, is refused with a bare 415 and
/// deletes nothing
/// Clause: 6.3.4, 5.6.10
#[gtest]
#[tokio::test]
async fn n_cmn_cnt_9014_batch_delete_declaring_text_plain_or_no_content_type_is_415() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).build()?;
    broker.entities().create(&device).send().await?.done()?;
    // The body goes as text/plain, which no typed operation sends, so it is sent as written.
    let request = request_to(&broker, Method::POST, "entityOperations/delete")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("text/plain"))
        .body(Bytes::from(format!("[\"urn:ngsi-ld:Device:{namespace}-1\"]")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE).without_body());
    // The body goes without a Content-Type, which no typed operation sends, so it is sent as written.
    let request_2 = request_to(&broker, Method::POST, "entityOperations/delete")?.body(Bytes::from(format!("[\"urn:ngsi-ld:Device:{namespace}-1\"]")));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE).without_body());
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    Ok(())
}
