//! HTTP binding details that diverged from ETSI GS CIM 009 v1.9.1.
//!
//! Three defects, each asserted against the clause that fixes its behaviour:
//! the clause 6.3.10 temporal `Content-Range` (its range unit, its size under
//! `lastN`, and its forwards `range-start`), the clause 5.6.2.5 list of
//! attributes actually updated, and the clause 6.3.8 placement of a `GeoJSON`
//! notification's `@context`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{notification_server::NotificationServer, server_config::MockServerConfig};
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use std::time::Duration;

/// The `Content-Range` field value of a response, as the broker rendered it.
fn content_range(headers: &HeaderMap) -> Option<String> {
    headers.get("content-range").and_then(|value| value.to_str().ok()).map(str::to_owned)
}

/// The broker's `temporal.default_last_n` safety cap. A page is only a 206 --
/// and only then carries a `Content-Range` -- when the result is actually cut,
/// which needs more observations than this and a `lastN` above it.
const SYSTEM_LAST_N_CAP: u32 = 19;

/// A temporal entity carrying `count` observations of `speed`, one per minute
/// from 2020-01-01T00:00:00Z, so a `lastN` page has a predictable range.
fn temporal_vehicle(id: &str, count: u32) -> Value {
    let instances: Vec<Value> = (0..count)
        .map(|minute| {
            json!({
                "type": "Property",
                "value": f64::from(minute),
                "observedAt": format!("2020-01-01T00:{minute:02}:00Z")
            })
        })
        .collect();
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": instances,
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Clause 6.3.10: `"unit" shall be equal to "DateTime"`. The broker previously
/// emitted the kebab-cased `date-time`, which is not the token the clause names
/// and which an RFC 7233 client keyed on the unit would not recognise.
#[tokio::test]
async fn regression_content_range_uses_the_datetime_range_unit() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:content-range-unit");

    let created = client
        .create_temporal_entity(&temporal_vehicle(&id, SYSTEM_LAST_N_CAP + 6))
        .await
        .expect("create temporal entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    // A `lastN` above the safety cap over more observations than the cap is
    // what truncates the result, and only a truncated page is a 206 carrying a
    // `Content-Range`.
    let response = client
        .get_temporal_entity(&id, "timerel=after&timeAt=2019-12-31T00:00:00Z&lastN=25", None)
        .await
        .expect("temporal retrieve failed");
    assert_eq!(response.status, StatusCode::PARTIAL_CONTENT, "a truncated page is 206: {:?}", response.body);

    let range = content_range(&response.headers).expect("a truncated page carries a Content-Range");
    assert!(range.starts_with("DateTime "), "clause 6.3.10 mandates the 'DateTime' range unit, got: {range}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.10: with `lastN` present, `"size" shall be equal to the requested
/// lastN`. The broker previously reported the number of stored instances it
/// happened to find, so a client could not tell how large a page it had asked
/// for.
#[tokio::test]
async fn regression_content_range_size_is_the_requested_last_n() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:content-range-size");

    let created = client
        .create_temporal_entity(&temporal_vehicle(&id, SYSTEM_LAST_N_CAP + 6))
        .await
        .expect("create temporal entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    // The client asks for 25 and the broker's cap narrows the page to 19. The
    // clause reports the number the client asked for, not the number found.
    let response = client
        .get_temporal_entity(&id, "timerel=after&timeAt=2019-12-31T00:00:00Z&lastN=25", None)
        .await
        .expect("temporal retrieve failed");
    assert_eq!(response.status, StatusCode::PARTIAL_CONTENT, "a truncated page is 206: {:?}", response.body);

    let range = content_range(&response.headers).expect("a truncated page carries a Content-Range");
    let size = range.rsplit_once('/').expect("a Content-Range carries a size").1.to_owned();
    assert_eq!(size, "25", "clause 6.3.10: size shall equal the requested lastN, got: {range}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.10, forwards pagination: without `lastN`, `"range-start" shall be
/// equal to timeAt for requests with timerel=after or timerel=between, or the
/// least recent timestamp in the range of the response, for requests with
/// timerel=before`. The broker previously used the request's `timeAt` for
/// `timerel=before` too, reporting a bound the page does not begin at.
#[tokio::test]
async fn regression_content_range_start_for_timerel_before_is_the_least_recent_timestamp() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:content-range-before");

    let created = client
        .create_temporal_entity(&temporal_vehicle(&id, SYSTEM_LAST_N_CAP + 6))
        .await
        .expect("create temporal entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    // `timeAt` is far later than any observation, so a range-start taken from
    // the request would be 2021, while the response itself begins in 2020.
    let response = client
        .get_temporal_entity(&id, "timerel=before&timeAt=2021-01-01T00:00:00Z", None)
        .await
        .expect("temporal retrieve failed");
    assert_eq!(response.status, StatusCode::PARTIAL_CONTENT, "a truncated page is 206: {:?}", response.body);

    let range = content_range(&response.headers).expect("a truncated page carries a Content-Range");
    let start = range
        .strip_prefix("DateTime ")
        .and_then(|rest| rest.split_once("Z-"))
        .map(|(start, _)| start.to_owned())
        .expect("a Content-Range carries a range-start");
    assert!(
        start.starts_with("2020-01-01T00:"),
        "clause 6.3.10: range-start for timerel=before is the least recent timestamp in the response, not the request timeAt, got: {range}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.2.4 defines Update Attributes as a partial update patch: an
/// attribute the target entity already carries is replaced, and one it does not
/// carry "shall be appended to the target Entity". Every named attribute is
/// therefore written, which clause 5.6.2.5 and Table 6.6.3.2-2 answer with 204
/// -- "All the Attributes were updated successfully". The 207 `UpdateResult`
/// is reserved for the case where some part could not be written, which locally
/// cannot arise and distributedly is a forwarded failure carrying a
/// `registrationId` (clause 5.2.19).
#[tokio::test]
async fn regression_update_attributes_appends_an_attribute_the_entity_did_not_carry() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:update-result");

    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 10},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = client.post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    // `speed` exists and is replaced; `brandName` does not and is appended.
    let fragment = json!({
        "speed": {"type": "Property", "value": 55},
        "brandName": {"type": "Property", "value": "Mercedes"},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = client.update_entity_attrs(&id, &fragment).await.expect("update attrs failed");

    assert_eq!(
        response.status,
        StatusCode::NO_CONTENT,
        "clause 5.6.2.4 appends the absent attribute, so every named attribute was written: {:?}",
        response.body
    );

    let stored = client
        .get_entity(&id, "", "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve entity failed");
    let body = stored.json_value().expect("the entity is JSON");
    assert_eq!(body["speed"]["value"], json!(55), "the existing attribute was replaced: {body}");
    assert_eq!(
        body["brandName"]["value"],
        json!("Mercedes"),
        "clause 5.6.2.4: the attribute the entity did not carry was appended: {body}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.2.5: when every attribute named is written, the operation answers
/// 204 and carries no `UpdateResult` at all.
#[tokio::test]
async fn regression_update_attributes_answers_no_content_when_every_attribute_is_updated() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:update-result-complete");

    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 10},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = client.post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let fragment = json!({
        "speed": {"type": "Property", "value": 55},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = client.update_entity_attrs(&id, &fragment).await.expect("update attrs failed");

    assert_eq!(
        response.status,
        StatusCode::NO_CONTENT,
        "clause 5.6.2.5: updating every named attribute answers 204: {:?}",
        response.body
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.3.5: an append reports the attributes it actually appended, which
/// is the set the request named -- not every attribute the merged entity ends
/// up carrying. The broker previously reported the whole entity, so an append
/// of one attribute to an entity holding ten claimed to have appended eleven.
#[tokio::test]
async fn regression_append_attributes_reports_only_the_attributes_the_request_named() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:append-result");

    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 10},
        "brandName": {"type": "Property", "value": "Mercedes"},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = client.post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    // `speed` already exists, so under `noOverwrite` it is refused; `colour` is
    // new and is appended. The pre-existing `brandName` is named by neither and
    // must appear in neither list.
    let fragment = json!({
        "speed": {"type": "Property", "value": 99},
        "colour": {"type": "Property", "value": "red"},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = client
        .raw(Method::POST, &format!("entities/{id}/attrs?options=noOverwrite"))
        .header("Content-Type", "application/ld+json")
        .json(&fragment)
        .send()
        .await
        .expect("append attrs failed");

    assert_eq!(
        response.status,
        StatusCode::MULTI_STATUS,
        "clause 5.6.3.5: appending only some attributes answers 207: {:?}",
        response.body
    );
    let body = response.json_value().expect("the 207 body is JSON");

    let updated = body["updated"].as_array().expect("clause 5.2.18 UpdateResult carries an 'updated' array");
    assert_eq!(updated.len(), 1, "only the newly appended attribute is reported: {body}");
    assert!(
        updated[0].as_str().is_some_and(|name| name.ends_with("colour")),
        "the appended attribute is reported: {body}"
    );
    assert!(
        !updated.iter().any(|name| name.as_str().is_some_and(|name| name.ends_with("brandName"))),
        "an attribute this request never named is not reported as appended: {body}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.8: "If the target MIME type is `application/geo+json` and the
/// `notification.endpoint.receiverInfo` contains a key `Prefer` whose value is
/// set to `body=ld+json` or the `Prefer` key is omitted or
/// `notification.endpoint.receiverInfo` does not exist, then the HTTP
/// notification request includes an `@context` element in the payload body."
/// The broker previously always used a `Link` header for `GeoJSON`, which is the
/// clause 6.3.7 rule for responses, not the clause 6.3.8 rule for notifications.
#[tokio::test]
async fn regression_geo_json_notification_carries_its_context_in_the_body_by_default() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification server must start");

    let subscription = json!({
        "id": scope.id("Subscription:geojson-body-context"),
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "notification": {
            "endpoint": {
                "uri": receiver.endpoint().expect("notification endpoint must be valid"),
                "accept": "application/geo+json"
            }
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = client.create_subscription(&subscription).await.expect("create subscription failed");
    assert_eq!(created.status, StatusCode::CREATED, "subscription create: {:?}", created.body);

    let entity = json!({
        "id": scope.id("Vehicle:geojson-body-context"),
        "type": "Vehicle",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let stored = client.post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(stored.status, StatusCode::CREATED, "create: {:?}", stored.body);

    let captured = receiver.next_request(Duration::from_secs(15)).await.expect("matching notification must arrive");
    let body: Value = captured.json().expect("the notification body is JSON");

    assert!(
        body.get("@context").is_some(),
        "clause 6.3.8: a geo+json notification with no receiverInfo carries its @context in the body: {body}"
    );
    assert!(
        captured.headers.get("link").is_none(),
        "the context is in the body, so no Link header advertises it as well"
    );

    receiver.shutdown().await.expect("notification server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.8: "If the target MIME type is `application/geo+json` and the
/// `notification.endpoint.receiverInfo` member contains a key `Prefer` whose
/// value is set to `body=json` then the HTTP notification request shall include
/// a Link header with a reference to the corresponding JSON-LD `@context`."
#[tokio::test]
async fn regression_geo_json_notification_preferring_json_carries_its_context_in_a_link_header() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification server must start");

    let subscription = json!({
        "id": scope.id("Subscription:geojson-link-context"),
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "notification": {
            "endpoint": {
                "uri": receiver.endpoint().expect("notification endpoint must be valid"),
                "accept": "application/geo+json",
                "receiverInfo": [{"key": "Prefer", "value": "body=json"}]
            }
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = client.create_subscription(&subscription).await.expect("create subscription failed");
    assert_eq!(created.status, StatusCode::CREATED, "subscription create: {:?}", created.body);

    let entity = json!({
        "id": scope.id("Vehicle:geojson-link-context"),
        "type": "Vehicle",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let stored = client.post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(stored.status, StatusCode::CREATED, "create: {:?}", stored.body);

    let captured = receiver.next_request(Duration::from_secs(15)).await.expect("matching notification must arrive");
    let body: Value = captured.json().expect("the notification body is JSON");

    let link = captured
        .headers
        .get("link")
        .expect("clause 6.3.8: Prefer body=json puts the geo+json context in a Link header");
    assert!(
        link.to_str().expect("ascii header").contains("json-ld#context"),
        "the Link header references the JSON-LD context: {link:?}"
    );
    assert!(
        body.get("@context").is_none(),
        "the context is in the Link header, so the body does not repeat it: {body}"
    );

    receiver.shutdown().await.expect("notification server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}
