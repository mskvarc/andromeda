//! Clause 4.23 ordering on the `createdAt` and `modifiedAt` system timestamps.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.23 (Entity Ordering) sorts a query result by
//! the `orderBy` terms, and clause 4.8 defines `createdAt` and `modifiedAt` as
//! temporal properties of every NGSI-LD Element. A broker that already sorts a
//! Context Source Registration query and a Subscription query by their creation
//! time has the same two timestamps on an Entity; clause 4.23 makes them
//! orderable there too.
//!
//! The two keywords read the entity's own `created_at` and the document row's
//! `modified_at` columns, like `id`, `type` and `scope` read theirs, rather than
//! resolving as attribute paths: a path lookup would find nothing, so every
//! entity would tie and the result would fall through to the `id ASC` pagination
//! tie-break instead of creation order.
//!
//! A dotted path whose head happens to be `createdAt` (`name.createdAt`) still
//! descends into an attribute's temporal sub-member, so admitting the bare
//! keyword does not change a path ordering.
//!
//! Every query carries `local=true`: clause 4.23.1 forbids ordering on
//! distributed operations, so without it these assertions could pass for the
//! wrong reason.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

/// The entity type every fixture in this module carries.
const FIXTURE_TYPE: &str = "Timestamped";

/// Create one entity and assert it took.
async fn create(client: &TestClient, id: &str) {
    let entity = json!({
        "id": id,
        "type": FIXTURE_TYPE,
        "name": {"type": "Property", "value": format!("value for {id}")},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = client.post_entity(&entity).with_ld_json().send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED, "create entity: {:?}", response.body);
}

/// Query this test's own entities, ordered by `order_by`.
///
/// The candidate set is pinned to `ids` because the suite runs its tests
/// concurrently against one database: an unrestricted query would order a
/// sibling test's fixtures alongside these.
async fn ordered_ids(client: &TestClient, ids: &[String], order_by: &str) -> Vec<String> {
    let query = format!(
        "type={FIXTURE_TYPE}&local=true&id={}&orderBy={}",
        ids.iter().map(|id| urlencoding::encode(id).into_owned()).collect::<Vec<_>>().join(","),
        urlencoding::encode(order_by)
    );
    let response = client
        .query_entities(&query, "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query_entities failed");
    let body = response.json_value().expect("query response must be valid JSON");
    assert_eq!(response.status, StatusCode::OK, "ordered query by {order_by}: {body}");
    body.as_array()
        .expect("response must be a JSON array")
        .iter()
        .map(|entity| entity["id"].as_str().expect("entity must have an id member").to_owned())
        .collect()
}

/// Create three entities in separate requests, with a short gap so each takes a
/// distinct `created_at`. Returns the ids in creation order.
async fn create_in_sequence(scope: &TestEnvironment, client: &TestClient) -> Vec<String> {
    let mut ids = Vec::with_capacity(3);
    for suffix in ["first", "second", "third"] {
        let id = scope.id(&format!("{FIXTURE_TYPE}:{suffix}"));
        create(client, &id).await;
        // Distinct requests already take distinct microsecond timestamps; the
        // gap makes the ordering robust rather than relying on that.
        sleep(Duration::from_millis(10)).await;
        ids.push(id);
    }
    ids
}

/// Clause 4.23 with clause 4.8: `orderBy=createdAt` sorts by creation time, so
/// entities created in separate requests come back in the order they were
/// created, and the descending term reverses that.
#[tokio::test]
async fn regression_ordering_created_at_sorts_by_creation_time() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_in_sequence(&scope, client).await;

    let ascending = ordered_ids(client, &ids, "createdAt").await;
    assert_eq!(ascending, ids, "createdAt sorts in creation order");

    let descending = ordered_ids(client, &ids, "createdAt;desc").await;
    let mut reversed = ids.clone();
    reversed.reverse();
    assert_eq!(descending, reversed, "createdAt;desc reverses creation order");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.23 with clause 4.8: `orderBy=modifiedAt` sorts by last-modified
/// time, so an entity whose payload is rewritten after the others sorts last in
/// ascending order.
#[tokio::test]
async fn regression_ordering_modified_at_sorts_by_last_modified_time() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_in_sequence(&scope, client).await;

    // Rewrite the first entity's payload, making its modifiedAt the newest.
    let bumped = &ids[0];
    let appended = client
        .append_entity_attrs(
            bumped,
            &json!({
                "extra": {"type": "Property", "value": 1},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await
        .expect("append attribute failed");
    assert_eq!(appended.status, StatusCode::NO_CONTENT, "append: {:?}", appended.body);

    let ascending = ordered_ids(client, &ids, "modifiedAt").await;
    assert_eq!(
        ascending,
        vec![ids[1].clone(), ids[2].clone(), ids[0].clone()],
        "the re-modified entity sorts last by modifiedAt"
    );

    scope.finish().await.expect("cleanup failed");
}

/// A dotted path whose head is `createdAt` is an attribute path into a temporal
/// sub-member, not the entity's own `createdAt` column, so admitting the bare
/// keyword leaves it a well-formed attribute ordering rather than a refusal.
#[tokio::test]
async fn regression_ordering_a_dotted_created_at_path_stays_an_attribute_ordering() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = create_in_sequence(&scope, client).await;

    // `name.createdAt` addresses the `createdAt` sub-member of the `name`
    // attribute, which no fixture carries, so every entity ties on it and the
    // result falls to the `id ASC` tie-break -- a 200, not a 400.
    let ordered = ordered_ids(client, &ids, "name.createdAt").await;
    let mut by_id = ids.clone();
    by_id.sort();
    assert_eq!(ordered, by_id, "a missing sub-member ties and falls to id order");

    scope.finish().await.expect("cleanup failed");
}
