//! A notification whose inline join walks a chain of 1,000 and of 2,000 Linked
//! Entities.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.2.14.1 gives a Subscription's notification
//! `join` and `joinLevel`, and clause 4.5.23 sets no cap on the level, so an
//! inline join nests as deep as the data goes. The notification is written and
//! released as bytes, and a chain this deep is far beyond what a recursive
//! writer, reader or destructor copes with on a worker thread, so the delivery
//! has to arrive whole. The body is checked as bytes: it exceeds a JSON
//! reader's recursion limit, so the nesting is counted by scanning the text.
//!
//! Needs the host broker: the cases drive the HTTP API and a callback receiver,
//! and are not run without one.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{notification_server::NotificationServer, server_config::MockServerConfig};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

/// Entities per `entityOperations/create` request.
const PAGE: usize = 500;

/// The member a nested Linked Entity sits under (clause 4.5.23).
const NESTED_MEMBER: &[u8] = br#""entity":"#;

/// How long the delivery of the whole chain is waited for.
const DELIVERY: Duration = Duration::from_secs(120);

/// One chain entity: it names `target` through `linkedTo` when there is one.
fn chain_entity(id: &str, target: Option<&str>) -> Value {
    let mut entity = json!({"id": id, "type": "Building", "@context": [CORE_CONTEXT]});
    if let Some(target) = target {
        entity["linkedTo"] = json!({"type": "Relationship", "object": target});
    }
    entity
}

/// Clause 5.2.14.1: a change to the chain head notifies the subscriber with the
/// whole chain nested under `entity` members, `depth` levels deep, and the
/// subscription asks for exactly that with `join=inline&joinLevel=depth`.
async fn assert_chain_is_delivered_inline(depth: usize) {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification receiver must start");
    let ids: Vec<String> = (0..=depth).map(|index| scope.id(&format!("Building:deep-notified-{depth}-{index}"))).collect();
    let entities: Vec<Value> = ids
        .iter()
        .enumerate()
        .map(|(index, id)| chain_entity(id, ids.get(index + 1).map(String::as_str)))
        .collect();
    for page in entities.chunks(PAGE) {
        // clone: the request body owns its entities while the chunked fixture is borrowed.
        let created = client.batch_create(&Value::Array(page.to_vec())).await.expect("batch create request");
        assert_eq!(created.status, StatusCode::CREATED, "batch create: {:?}", created.body);
    }

    let subscription_id = scope.id(&format!("Subscription:deep-notified-{depth}"));
    let subscribed = client
        .create_subscription(&json!({
            "id": subscription_id,
            "type": "Subscription",
            "entities": [{"type": "Building", "id": ids[0]}],
            "notification": {
                "endpoint": {"uri": receiver.endpoint_url().expect("notification endpoint must be valid"), "accept": "application/json"},
                "join": "inline",
                "joinLevel": depth
            },
            "@context": [CORE_CONTEXT]
        }))
        .await
        .expect("create subscription request");
    assert_eq!(subscribed.status, StatusCode::CREATED, "create subscription: {:?}", subscribed.body);
    receiver.drain_requests().await;

    let appended = client
        .append_entity_attrs(&ids[0], &json!({"name": {"type": "Property", "value": "Touched"}, "@context": [CORE_CONTEXT]}))
        .await
        .expect("append attributes request");
    assert_eq!(appended.status, StatusCode::NO_CONTENT, "append: {:?}", appended.body);

    let notification = receiver.next_request(DELIVERY).await.expect("the joined notification is delivered");
    client.delete_subscription(&subscription_id).await.expect("delete subscription");
    receiver.shutdown().await.expect("notification receiver shutdown failed");

    let body = notification.body.as_ref();
    let nested = body.windows(NESTED_MEMBER.len()).filter(|window| *window == NESTED_MEMBER).count();
    assert_eq!(nested, depth, "every link of the chain is nested under the delivered entity");
    let last = format!(r#""id":"{}""#, ids[depth]);
    assert!(
        body.windows(last.len()).any(|window| window == last.as_bytes()),
        "the last link of the chain is nested in the delivery"
    );

    scope.finish().await.expect("environment cleanup");
}

/// Follow-up 50: a 1,000-level inline join is delivered whole.
#[tokio::test]
async fn regression_a_1000_level_inline_join_notification_is_delivered() {
    assert_chain_is_delivered_inline(1000).await;
}

/// Follow-up 50: a 2,000-level inline join is delivered whole.
#[tokio::test]
async fn regression_a_2000_level_inline_join_notification_is_delivered() {
    assert_chain_is_delivered_inline(2000).await;
}
