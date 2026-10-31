//! Query temporal entities by post declaring merge patch json is 415.

use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: Query Temporal Evolution of Entities by POST declaring application/merge-patch+json is refused with a bare
/// 415
/// Clause: 6.3.4, 5.7.4
/// Recorded: 6.3.4 allows application/merge-patch+json for PATCH requests only; any other media type than
/// application/json and application/ld+json is 415 without a body
#[gtest]
#[tokio::test]
async fn n_cmn_cnt_2012_query_temporal_entities_by_post_declaring_merge_patch_json_is_415() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // The body goes as application/merge-patch+json, which no typed operation sends, so it is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "temporal/entityOperations/query")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/merge-patch+json")).body(Bytes::from(format!("{{ \"type\": \"Query\", \"entities\": [ {{ \"type\": \"Device_{namespace}\" }} ], \"temporalQ\": {{ \"timerel\": \"after\", \"timeAt\": \"2020-01-01T00:00:00Z\" }} }}")));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE).without_body());
    Ok(())
}
