//! Identifier bounds: every stored identifier shares one octet bound.
//!
//! Clause 4.6.2 sets no length on a URI, but every identifier the broker
//! stores is a B-tree key, so Andromeda bounds each one -- an Entity id, an
//! Entity type, a `datasetId`, an `instanceId`, a Subscription or Registration
//! id and an `idPattern` -- at the same number of UTF-8 octets. The bound
//! applies to the value as stored, which for a term is its expansion: a short
//! compacted token an inline `@context` maps to an over-long IRI is refused
//! rather than silently dropped or retargeted. A value at the bound is stored
//! and stays addressable by every access path.
//!
//! A Relationship object is a reference, not an identifier: it is bounded
//! only by the request-body limit. One no longer than an identifier sits in
//! the traversed `value_uri` column, a longer one in `value_long_uri`.

use crate::{
    bad_request::assert_bad_request_data,
    bounded_identifier::high_entropy_identifier,
    projection_rows::{entity_key, rows_for_attribute, temporal_entity_exists, uri_placement},
};
use andromeda_ngsi_ld_lexicon::identifier_length::IDENTIFIER_BYTE_LIMIT;
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use urlencoding::encode;

/// The octet length one past the identifier bound.
const PAST_THE_LIMIT: usize = IDENTIFIER_BYTE_LIMIT + 1;

/// A notification or Context Source endpoint nothing listens on; every request
/// naming it is refused before the broker would ever call it.
const UNREACHABLE_ENDPOINT: &str = "http://127.0.0.1:9/ngsi-ld";

/// Create one Entity and require the broker to admit it.
async fn create(client: &TestClient, entity: &Value) {
    let created = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// The identifiers of the Entities one query answered with, in response order.
async fn queried_ids(client: &TestClient, query: &str) -> Vec<String> {
    let response = client
        .query_entities(query, "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query entities failed");
    assert_eq!(response.status, StatusCode::OK, "query {query}: {:?}", response.body);
    let body = response.json_value().expect("query response must be JSON");
    body.as_array()
        .expect("query response must be an array")
        .iter()
        .filter_map(|entity| entity["id"].as_str().map(str::to_owned))
        .collect()
}

#[tokio::test]
async fn an_expanded_type_past_the_limit_is_refused_rather_than_dropped() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:long-expanded-type");
    let long_type = high_entropy_identifier("http://example.test/types/", PAST_THE_LIMIT, 21);

    // The compacted token is short; only its expansion through the inline
    // context crosses the bound, and clause 4.5.1 requires at least one type,
    // so dropping it would leave an Entity the request never described.
    let entity = json!({
        "id": id,
        "type": "LongType",
        "@context": [TEST_SUITE_CONTEXT, {"LongType": long_type}]
    });
    let refused = client.post_entity(&entity).send().await.expect("create entity request failed");
    assert_bad_request_data(&refused, "creating an Entity whose expanded type is past the bound");
    assert_eq!(entity_key(scope.database_pool(), &id).await, None, "the refused Entity must not be stored");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn an_expanded_dataset_id_past_the_limit_is_refused_rather_than_retargeted() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:long-expanded-dataset");

    create(
        client,
        &json!({
            "id": id,
            "type": "Building",
            "temperature": {"type": "Property", "value": 1},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    // `ds:x` is a compact IRI; the inline context makes its expansion one
    // octet longer than the prefix IRI, which is already at the bound.
    let long_prefix = format!("{}/", high_entropy_identifier("http://example.test/datasets/", IDENTIFIER_BYTE_LIMIT, 22));
    let fragment = json!({
        "temperature": {"type": "Property", "value": 2, "datasetId": "ds:x"},
        "@context": [TEST_SUITE_CONTEXT, {"ds": long_prefix}]
    });
    let refused = client.append_entity_attrs(&id, &fragment).await.expect("append attributes request failed");
    assert_bad_request_data(&refused, "appending an instance whose expanded datasetId is past the bound");

    // Clause 4.5.5: an instance is identified by its datasetId, so a refused
    // datasetId that fell back to the default would overwrite the default
    // instance instead of adding a named one.
    let rows = rows_for_attribute(scope.database_pool(), &id, "temperature").await;
    assert_eq!(rows.len(), 1, "the refused instance must add no grain: {rows:?}");
    assert_eq!(rows[0].dataset_id, None, "the one grain must remain the default instance");
    assert_eq!(rows[0].value_text.as_deref(), Some("1"), "the default instance must keep its value");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn an_instance_id_past_the_limit_is_refused() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:long-instance-id");
    let instance_id = high_entropy_identifier("urn:ngsi-ld:Instance:", PAST_THE_LIMIT, 23);

    // Clause 4.5.7: a client-supplied `instanceId` identifies the instance and
    // is stored as given, so it is bounded like every other identifier.
    let temporal_entity = json!({
        "id": id,
        "type": "Building",
        "temperature": [{
            "type": "Property",
            "value": 1,
            "observedAt": "2026-01-01T00:00:00Z",
            "instanceId": instance_id
        }],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let refused = client
        .create_temporal_entity(&temporal_entity)
        .await
        .expect("create temporal entity request failed");
    assert_bad_request_data(&refused, "creating a Temporal Entity whose instanceId is past the bound");
    assert!(
        !temporal_entity_exists(scope.database_pool(), &id).await,
        "the refused Temporal Entity must not be stored"
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn a_relationship_object_at_the_limit_is_projected_and_traversable() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let target = high_entropy_identifier(&scope.id("Building:link-target:"), IDENTIFIER_BYTE_LIMIT, 24);
    let source = scope.id("Building:link-source");

    create(
        client,
        &json!({
            "id": target,
            "type": "Building",
            "name": {"type": "Property", "value": "x"},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    create(
        client,
        &json!({
            "id": source,
            "type": "Building",
            "link": {"type": "Relationship", "object": target},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let rows = rows_for_attribute(scope.database_pool(), &source, "link").await;
    assert_eq!(rows.len(), 1, "the Relationship must be one grain: {rows:?}");
    assert_eq!(
        (rows[0].attribute_kind, rows[0].value_kind),
        (2, 7),
        "the Relationship must be projected as a URI"
    );
    assert_eq!(
        rows[0].value_text.as_deref(),
        Some(target.as_str()),
        "the object at the bound must be projected whole"
    );

    // Clause 4.9: an equality on the Relationship compares its object.
    let equality = scope.scope_query(&format!("type=Building&q={}", encode(&format!("link==\"{target}\""))));
    assert_eq!(
        queried_ids(client, &equality).await,
        vec![source.as_str()],
        "an equality on the object at the bound must select the source"
    );

    // Clause 4.9 `rel{...}`: the hop lands on the target through the object
    // at the bound, and clause 5.7.2.4 admits it under a retrieving join.
    let traversal = scope.scope_query(&format!("type=Building&q={}&join=inline&joinLevel=1", encode("link{name==\"x\"}")));
    assert_eq!(
        queried_ids(client, &traversal).await,
        vec![source.as_str()],
        "a hop through the object at the bound must select the source"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.6.2 sets no length on a URI, and a Relationship object is a
/// reference rather than an identifier: one past the identifier bound is
/// stored, under the URI value kind, in the overflow column.
#[tokio::test]
async fn a_relationship_object_past_the_limit_is_projected_into_the_overflow_column() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:long-link-source");
    let object = high_entropy_identifier(&scope.id("Building:long-link-target:"), PAST_THE_LIMIT, 25);

    create(
        client,
        &json!({
            "id": id,
            "type": "Building",
            "link": {"type": "Relationship", "object": object},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let rows = rows_for_attribute(scope.database_pool(), &id, "link").await;
    assert_eq!(rows.len(), 1, "the Relationship must be one grain: {rows:?}");
    assert_eq!(
        (rows[0].attribute_kind, rows[0].value_kind),
        (2, 7),
        "the Relationship must be projected as a URI"
    );
    assert_eq!(
        rows[0].value_text.as_deref(),
        Some(object.as_str()),
        "the object past the bound must be projected whole"
    );
    assert_eq!(
        uri_placement(scope.database_pool(), &id, "link").await,
        (None, Some(object.clone())),
        "the object past the bound must sit in the overflow column"
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn query_and_path_identifiers_past_the_limit_are_bad_request_data() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let long_id = high_entropy_identifier(&scope.id("Building:long-path:"), PAST_THE_LIMIT, 26);

    let retrieved = client
        .get_entity(&long_id, "", "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve entity request failed");
    assert_bad_request_data(&retrieved, "retrieving an Entity by an id past the bound");

    let queried = client
        .query_entities(&format!("id={}&type=Building", encode(&long_id)), "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query entities request failed");
    assert_bad_request_data(&queried, "querying Entities by an id past the bound");

    let subscription = json!({
        "id": high_entropy_identifier(&scope.id("subscription:long-id:"), PAST_THE_LIMIT, 27),
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "notification": {"endpoint": {"uri": UNREACHABLE_ENDPOINT}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let subscribed = client.create_subscription(&subscription).await.expect("create subscription request failed");
    assert_bad_request_data(&subscribed, "creating a Subscription whose id is past the bound");

    let registration = json!({
        "id": high_entropy_identifier(&scope.id("registration:long-id:"), PAST_THE_LIMIT, 28),
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Building"}]}],
        "endpoint": UNREACHABLE_ENDPOINT,
        "@context": [TEST_SUITE_CONTEXT]
    });
    let registered = client.create_registration(&registration).await.expect("create registration request failed");
    assert_bad_request_data(&registered, "creating a Registration whose id is past the bound");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn a_registration_id_pattern_past_the_limit_is_refused() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id_pattern = high_entropy_identifier("^urn:ngsi-ld:Building:", PAST_THE_LIMIT, 29);

    let registration = json!({
        "id": scope.id("registration:long-id-pattern"),
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"idPattern": id_pattern, "type": "Building"}]}],
        "endpoint": UNREACHABLE_ENDPOINT,
        "@context": [TEST_SUITE_CONTEXT]
    });
    let refused = client.create_registration(&registration).await.expect("create registration request failed");
    assert_bad_request_data(&refused, "creating a Registration whose idPattern is past the bound");

    scope.finish().await.expect("cleanup failed");
}
