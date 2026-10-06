#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
use crate::helpers::{BAD_REQUEST_DATA_TYPE, client, create_subscription, default_subscription};
use andromeda_test_harness::environment::TestEnvironment;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// Create the default subscription with `members` set over it, assert it is
/// refused as `BadRequestData` (clause 5.8.1.4) and that the problem names
/// `named`.
async fn assert_refused_naming(case: &str, members: Value, named: &str) {
    let http_client = client();
    let scope = TestEnvironment::from_suite("subscription").await.expect("isolated test environment");
    let subscription_id = scope.id(&format!("Subscription:member-validation-{case}"));
    let mut payload = default_subscription(&subscription_id);
    let object = payload.as_object_mut().expect("payload object");
    object.remove("timeInterval");
    for (member, value) in members.as_object().expect("members object") {
        object.insert(member.clone(), value.clone());
    }

    let (status, _, body) = create_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST, "{case}: {body}");
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str(), "{case}: {body}");
    assert!(body.to_string().contains(named), "{case}: the problem names {named}: {body}");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn subscription_create_temporal_q_without_time_at_returns_bad_request_data() {
    assert_refused_naming("no-time-at", json!({"temporalQ": {"timerel": "after"}}), "timeAt").await;
}

#[tokio::test]
async fn subscription_create_temporal_q_without_timerel_returns_bad_request_data() {
    assert_refused_naming("no-timerel", json!({"temporalQ": {"timeAt": "2020-01-01T00:00:00Z"}}), "timerel").await;
}

#[tokio::test]
async fn subscription_create_temporal_q_between_without_end_time_at_returns_bad_request_data() {
    assert_refused_naming(
        "between-no-end",
        json!({"temporalQ": {"timerel": "between", "timeAt": "2020-01-01T00:00:00Z"}}),
        "endTimeAt",
    )
    .await;
}

#[tokio::test]
async fn subscription_create_geo_q_with_unknown_member_returns_bad_request_data() {
    assert_refused_naming(
        "geo-q-unknown",
        json!({"geoQ": {"georel": "within", "geometry": "Point", "coordinates": [1.0, 2.0], "unknown": 1}}),
        "unknown",
    )
    .await;
}

#[tokio::test]
async fn subscription_create_geo_q_with_non_string_geoproperty_returns_bad_request_data() {
    assert_refused_naming(
        "geo-q-geoproperty",
        json!({"geoQ": {"georel": "within", "geometry": "Point", "coordinates": [1.0, 2.0], "geoproperty": 5}}),
        "geoproperty",
    )
    .await;
}

#[tokio::test]
async fn subscription_create_non_boolean_is_active_returns_bad_request_data() {
    assert_refused_naming("is-active", json!({"isActive": "yes"}), "isActive").await;
}

#[tokio::test]
async fn subscription_create_non_boolean_local_only_returns_bad_request_data() {
    assert_refused_naming("local-only", json!({"localOnly": "yes"}), "localOnly").await;
}

#[tokio::test]
async fn subscription_create_non_boolean_split_entities_returns_bad_request_data() {
    assert_refused_naming("split-entities", json!({"splitEntities": 1}), "splitEntities").await;
}
