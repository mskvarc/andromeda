//! Batch creation and update declaring merge patch json are 415.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
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

/// Pins: the entity batch operations Create, Upsert, Update and Merge declaring application/merge-patch+json are
/// refused with a bare 415 and change nothing
/// Clause: 6.3.4
/// Recorded: 6.3.4 allows application/merge-patch+json for PATCH requests only; any other media type than
/// application/json and application/ld+json is 415 without a body
#[gtest]
#[tokio::test]
async fn n_cmn_cnt_2001_batch_creation_and_update_declaring_merge_patch_json_are_415() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device_2_id = namespace.urn("Device", 2)?;
    let device_3_id = namespace.urn("Device", 3)?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).property("speed", 1).build()?;
    broker.entities().create(&device).send().await?.done()?;
    // The body goes as application/merge-patch+json, which no typed operation sends, so it is sent as written.
    let request = request_to(&broker, Method::POST, "entityOperations/create")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/merge-patch+json"))
        .body(Bytes::from(format!(
            "[ {{ \"id\": \"urn:ngsi-ld:Device:{namespace}-2\", \"type\": \"Device_{namespace}\" }} ]"
        )));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE).without_body());
    // The body goes as application/merge-patch+json, which no typed operation sends, so it is sent as written.
    let request_2 = request_to(&broker, Method::POST, "entityOperations/upsert")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/merge-patch+json"))
        .body(Bytes::from(format!(
            "[ {{ \"id\": \"urn:ngsi-ld:Device:{namespace}-3\", \"type\": \"Device_{namespace}\" }} ]"
        )));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE).without_body());
    // The body goes as application/merge-patch+json, which no typed operation sends, so it is sent as written.
    let request_3 = request_to(&broker, Method::POST, "entityOperations/update")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/merge-patch+json"))
        .body(Bytes::from(format!(
            "[ {{ \"id\": \"urn:ngsi-ld:Device:{namespace}-1\", \"type\": \"Device_{namespace}\", \"speed\": {{ \"type\": \"Property\", \"value\": 9 }} }} ]"
        )));
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE).without_body());
    // The body goes as application/merge-patch+json, which no typed operation sends, so it is sent as written.
    let request_4 = request_to(&broker, Method::POST, "entityOperations/merge")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/merge-patch+json"))
        .body(Bytes::from(format!(
            "[ {{ \"id\": \"urn:ngsi-ld:Device:{namespace}-1\", \"type\": \"Device_{namespace}\", \"speed\": {{ \"type\": \"Property\", \"value\": 9 }} }} ]"
        )));
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE).without_body());
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(1)));
    let answer_5 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_2_id)
        .send()
        .await;
    expect_that!(&answer_5, answered(StatusCode::NOT_FOUND));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_3_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    Ok(())
}
