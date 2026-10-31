//! Create temporal evolution declaring merge patch json is 415.

use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: Create Temporal Evolution of an Entity (POST) declaring application/merge-patch+json is refused with a
/// bare 415 and stores nothing
/// Clause: 6.3.4, 5.6.11
/// Recorded: 6.3.4 allows application/merge-patch+json for PATCH requests only; any other media type than
/// application/json and application/ld+json is 415 without a body
#[gtest]
#[tokio::test]
async fn n_cmn_cnt_2010_create_temporal_evolution_declaring_merge_patch_json_is_415() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    // The body goes as application/merge-patch+json, which no typed operation sends, so it is sent as written.
    let request = request_to(&broker, Method::POST, "temporal/entities")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/merge-patch+json")).body(Bytes::from(format!("{{ \"id\": \"urn:ngsi-ld:Device:{namespace}-1\", \"type\": \"Device_{namespace}\", \"speed\": [ {{ \"type\": \"Property\", \"value\": 1, \"observedAt\": \"2020-01-01T00:00:00Z\" }} ] }}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE).without_body());
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&device_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
