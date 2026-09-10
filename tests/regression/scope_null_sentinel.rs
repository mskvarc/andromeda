//! The NGSI-LD Null as the value of the entity `scope` member.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.18 admits two things under one grammar: a
//! hierarchical scope path, and the special string `urn:ngsi-ld:null`, which
//! "shall be only used and only appear in case of deleted scopes". The broker
//! read both as paths and handed the sentinel to the label-path index its
//! current-state scopes are stored in, where `:` and `-` are not label
//! characters: every write verb answered a well-formed request with
//! `InternalError`. The same cast failed for a scope level longer than a label
//! can hold, on the write path and in the `scopeQ` of clause 4.19 alike.
//!
//! Clause 5.5.4 says which verbs may carry it at all: "Any attempt to use
//! "urn:ngsi-ld:null" as a first level member value ("<key>":
//! "urn:ngsi-ld:null"), with the exception of NGSI-LD Fragments (see clause
//! 5.4) used in partial update and merge operations (as mandated by clause
//! 5.5.8 and clause 5.5.12) [...] shall result in an error of type
//! `BadRequestData`". `scope` is a first level member (clause 5.2.4), so a
//! creation, a replacement and an append are refused, while the partial update
//! of clause 5.6.2 and the merge of clause 5.6.17 delete the member with it.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The problem type clause 5.5.4 gives data that is not syntactically correct
/// according to the API data type definitions.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// The NGSI-LD Null, which clause 4.18 spells a deleted scope with.
const NGSI_LD_NULL: &str = "urn:ngsi-ld:null";

/// A three-level scope path, the shape of clause 4.18 EXAMPLE 3.
const MULTI_LEVEL_SCOPE: &str = "/a/b/c";

/// Assert a refusal is the `BadRequestData` of clauses 5.5.3 and 5.5.4, and in
/// particular that it is not the `InternalError` this defect produced.
fn assert_bad_request_data(response: &TestResponse, scenario: &str) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{scenario}: {problem}");
}

/// One `Building` carrying `scope` and one ordinary Property.
fn scoped_building(id: &str, scope: &Value) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "scope": scope,
        "temperature": {"type": "Property", "value": 21.5},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Create one `Building` and assert clause 6.4.3.1 answered with 201.
async fn create_building(client: &TestClient, id: &str, scope: &Value) {
    let response = client
        .post_entity(&scoped_building(id, scope))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(response.status, StatusCode::CREATED, "create {id}: {:?}", response.body);
}

/// The `scope` member of the stored entity, absent when it carries none.
async fn stored_scope(client: &TestClient, id: &str) -> Option<Value> {
    let response = client
        .get_entity(id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve entity request");
    assert_eq!(response.status, StatusCode::OK, "retrieve {id}: {:?}", response.body);
    let body = response.json_value().expect("the retrieved entity is JSON");
    body.get("scope").cloned()
}

/// One fragment naming nothing but `scope`.
fn scope_fragment(scope: &Value) -> Value {
    json!({"scope": scope, "@context": [TEST_SUITE_CONTEXT]})
}

/// Clause 5.6.1 creates an Entity from a whole Entity representation rather
/// than from a Fragment, so clause 5.5.4's exception does not reach it: "Any
/// attempt to use "urn:ngsi-ld:null" as a first level member value [...] shall
/// result in an error of type `BadRequestData`". The broker answered 500, because
/// the sentinel was stored as if it were a scope path.
#[tokio::test]
async fn regression_scope_null_sentinel_create_refuses_the_deleted_scope_spelling() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Building:create-null-scope");
    let response = client
        .post_entity(&scoped_building(&id, &json!(NGSI_LD_NULL)))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_bad_request_data(&response, "clause 5.5.4 forbids a first level null on create");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.18.4 states it outright for Replace Entity: "The behaviour
/// defined in clause 5.5.4 on JSON-LD validation. NGSI-LD Nulls are not
/// supported by this operation." The scope the target already carries is left
/// standing, because the refusal precedes the write.
#[tokio::test]
async fn regression_scope_null_sentinel_replace_refuses_the_deleted_scope_spelling() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Building:replace-null-scope");
    create_building(client, &id, &json!(MULTI_LEVEL_SCOPE)).await;

    let response = client
        .replace_entity(&id, &scoped_building(&id, &json!(NGSI_LD_NULL)), "application/ld+json", None)
        .await
        .expect("replace entity request");
    assert_bad_request_data(&response, "clause 5.6.18.4 does not support nulls");
    assert_eq!(
        stored_scope(client, &id).await,
        Some(json!(MULTI_LEVEL_SCOPE)),
        "a refused replacement leaves the stored scope standing"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.3.4 folds a fragment's `scope` into the target's — "If scope is
/// included in the Fragment and overwrite is allowed, the scope of the target
/// Entity will become the one included in the Fragment" — and its validation
/// step names clause 5.5.4 alone, without the "NGSI-LD Nulls should be
/// supported by this operation" that clauses 5.6.2.4 and 5.6.17.4 add. An
/// append is neither a partial update nor a merge, so the sentinel is refused.
#[tokio::test]
async fn regression_scope_null_sentinel_append_attrs_refuses_the_deleted_scope_spelling() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Building:append-null-scope");
    create_building(client, &id, &json!(MULTI_LEVEL_SCOPE)).await;

    let response = client
        .append_entity_attrs(&id, &scope_fragment(&json!(NGSI_LD_NULL)))
        .await
        .expect("append attributes request");
    assert_bad_request_data(&response, "clause 5.5.4 forbids a first level null on append");
    assert_eq!(
        stored_scope(client, &id).await,
        Some(json!(MULTI_LEVEL_SCOPE)),
        "a refused append leaves the stored scope standing"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.2.4 sends Update Attributes through the partial update patch of
/// clause 5.5.8, which reads the sentinel as a deletion: "For each member of
/// the Fragment, whose value is an NGSI-LD Null, contained by the target, the
/// target member is deleted." Clause 4.18 spells a deleted scope with that same
/// string, so the member goes and the request succeeds.
#[tokio::test]
async fn regression_scope_null_sentinel_update_attrs_deletes_the_scope() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Building:update-null-scope");
    create_building(client, &id, &json!(MULTI_LEVEL_SCOPE)).await;

    let response = client
        .update_entity_attrs(&id, &scope_fragment(&json!(NGSI_LD_NULL)))
        .await
        .expect("update attributes request");
    assert_eq!(
        response.status,
        StatusCode::NO_CONTENT,
        "clause 5.5.8 deletes the member the fragment nulls: {:?}",
        response.body
    );
    assert_eq!(stored_scope(client, &id).await, None, "the deleted scope is gone rather than stored as a path");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.17.4 sends Merge Entity through the merge patch of clause 5.5.12,
/// whose own rule is the same: "For each member of the Fragment, whose value is
/// an NGSI-LD Null, contained by the target, the target member is removed."
#[tokio::test]
async fn regression_scope_null_sentinel_merge_deletes_the_scope() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Building:merge-null-scope");
    create_building(client, &id, &json!(MULTI_LEVEL_SCOPE)).await;

    let response = client
        .merge_entity(&id, &scope_fragment(&json!(NGSI_LD_NULL)), None)
        .await
        .expect("merge entity request");
    assert_eq!(
        response.status,
        StatusCode::NO_CONTENT,
        "clause 5.5.12 removes the member the fragment nulls: {:?}",
        response.body
    );
    assert_eq!(stored_scope(client, &id).await, None, "the deleted scope is gone rather than stored as a path");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.4.4: "If the target Attribute is scope, then an error of type
/// `BadRequestData` shall be raised." The scope of clause 4.18 is a non-reified
/// Entity member rather than an Attribute, so it is not a partial update
/// target — with the NGSI-LD Null no more than with a path.
#[tokio::test]
async fn regression_scope_null_sentinel_partial_update_refuses_scope_as_its_target() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Building:partial-update-scope-target");
    create_building(client, &id, &json!(MULTI_LEVEL_SCOPE)).await;

    for value in [json!(NGSI_LD_NULL), json!("/x")] {
        let fragment = json!({"type": "Property", "value": value, "@context": [TEST_SUITE_CONTEXT]});
        let response = client
            .partial_update_attr(&id, "scope", &fragment)
            .await
            .expect("partial update attribute request");
        assert_bad_request_data(&response, "clause 5.6.4.4 refuses scope as a partial update target");
    }
    assert_eq!(
        stored_scope(client, &id).await,
        Some(json!(MULTI_LEVEL_SCOPE)),
        "a refused partial update leaves the stored scope standing"
    );

    scope.finish().await.expect("cleanup failed");
}

/// The fix is a reading of the NGSI-LD Null, not a blanket refusal of `scope`:
/// a hierarchical path of clause 4.18 EXAMPLE 3 still round-trips through every
/// verb that writes one — create, append (clause 5.6.3.4), update (clause
/// 5.6.2.4) and merge (clause 5.6.17.4) — and comes back as it was written.
#[tokio::test]
async fn regression_scope_null_sentinel_a_multi_level_scope_round_trips_through_every_write_verb() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Building:multi-level-scope");
    create_building(client, &id, &json!(MULTI_LEVEL_SCOPE)).await;
    assert_eq!(stored_scope(client, &id).await, Some(json!(MULTI_LEVEL_SCOPE)), "a created scope reads back");

    let appended = client
        .append_entity_attrs(&id, &scope_fragment(&json!("/a/b/d")))
        .await
        .expect("append attributes request");
    assert!(
        appended.status.is_success(),
        "clause 5.6.3.4 folds a scope into the target: {:?}",
        appended.body
    );
    assert_eq!(
        stored_scope(client, &id).await,
        Some(json!("/a/b/d")),
        "an overwriting append takes the fragment's scope"
    );

    let updated = client
        .update_entity_attrs(&id, &scope_fragment(&json!("/a/b/e")))
        .await
        .expect("update attributes request");
    assert_eq!(updated.status, StatusCode::NO_CONTENT, "clause 5.6.2.4 replaces the scope: {:?}", updated.body);
    assert_eq!(stored_scope(client, &id).await, Some(json!("/a/b/e")));

    let merged = client
        .merge_entity(&id, &scope_fragment(&json!(MULTI_LEVEL_SCOPE)), None)
        .await
        .expect("merge entity request");
    assert_eq!(merged.status, StatusCode::NO_CONTENT, "clause 5.6.17.4 replaces the scope: {:?}", merged.body);
    assert_eq!(stored_scope(client, &id).await, Some(json!(MULTI_LEVEL_SCOPE)));

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.18 admits the special string only "in case of deleted scopes",
/// which is a statement about the whole member: one request cannot delete the
/// scope and set it at the same time. The mixed array reached the label-path
/// cast as two paths and produced an `InternalError`.
#[tokio::test]
async fn regression_scope_null_sentinel_a_deleted_scope_beside_a_path_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let created = scope.id("Building:mixed-scope-create");
    let mixed = json!([MULTI_LEVEL_SCOPE, NGSI_LD_NULL]);
    let response = client
        .post_entity(&scoped_building(&created, &mixed))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_bad_request_data(&response, "clause 4.18 admits the null only for a deleted scope");

    let merged = scope.id("Building:mixed-scope-merge");
    create_building(client, &merged, &json!(MULTI_LEVEL_SCOPE)).await;
    let response = client.merge_entity(&merged, &scope_fragment(&mixed), None).await.expect("merge entity request");
    assert_bad_request_data(&response, "a merge fragment cannot delete and set the scope at once");
    assert_eq!(stored_scope(client, &merged).await, Some(json!(MULTI_LEVEL_SCOPE)));

    scope.finish().await.expect("cleanup failed");
}

/// A scope level the label-path index cannot hold is bad input, not an internal
/// failure: clause 5.5.6 reserves `InternalError` for "an unexpected error
/// [that] happens and the operation cannot be completed", which a value the
/// broker can decide about before it writes anything is not. The level here is
/// one character past the widest label the store can carry, and every verb that
/// writes a scope must refuse it the same way.
#[tokio::test]
async fn regression_scope_null_sentinel_a_scope_level_beyond_the_storable_bound_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let oversized_level = format!("/{}", "a".repeat(1001));
    let oversized = json!(&oversized_level);

    let created = scope.id("Building:oversized-scope-create");
    let response = client
        .post_entity(&scoped_building(&created, &oversized))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_bad_request_data(&response, "an unstorable scope level is bad input on create");

    let target = scope.id("Building:oversized-scope-target");
    create_building(client, &target, &json!(MULTI_LEVEL_SCOPE)).await;
    for (verb, response) in [
        (
            "append",
            client.append_entity_attrs(&target, &scope_fragment(&oversized)).await.expect("append request"),
        ),
        (
            "update",
            client.update_entity_attrs(&target, &scope_fragment(&oversized)).await.expect("update request"),
        ),
    ] {
        assert_bad_request_data(&response, &format!("an unstorable scope level is bad input on {verb}"));
    }
    let merged = client
        .merge_entity(&target, &scope_fragment(&oversized), None)
        .await
        .expect("merge entity request");
    assert_bad_request_data(&merged, "an unstorable scope level is bad input on merge");
    assert_eq!(stored_scope(client, &target).await, Some(json!(MULTI_LEVEL_SCOPE)));

    // The same bound guards the read side: clause 4.19's `scopeQ` is answered
    // through the same label-path index, so a pattern carrying a level it
    // cannot hold is a malformed query rather than an internal failure.
    let queried = client
        .query_entities(
            &format!("type=Building&scopeQ={}", urlencoding::encode(&oversized_level)),
            "application/ld+json",
            Some(TEST_SUITE_CONTEXT),
        )
        .await
        .expect("query entities request");
    assert_bad_request_data(&queried, "an unstorable scope level is bad input in a scopeQ");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.7.4 runs the behaviour of Create Entity for each item of the
/// input array, so the clause 5.5.4 refusal reaches the batch form too — as a
/// per-entity `BatchEntityError` of clause 5.2.17 rather than as a failure of
/// the whole request, leaving the sibling that carries a real scope path to be
/// created.
#[tokio::test]
async fn regression_scope_null_sentinel_batch_create_refuses_the_deleted_scope_spelling() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let refused = scope.id("Building:batch-null-scope");
    let admitted = scope.id("Building:batch-path-scope");
    let body = json!([
        scoped_building(&refused, &json!(NGSI_LD_NULL)),
        scoped_building(&admitted, &json!(MULTI_LEVEL_SCOPE))
    ]);

    let response = client.batch_create(&body).await.expect("batch create request");
    assert_eq!(response.status, StatusCode::MULTI_STATUS, "batch create: {:?}", response.body);
    let result = response.json_value().expect("a batch answers with a clause 5.2.16 result");
    let errors = result["errors"].as_array().expect("the clause 5.2.16 result lists its errors");
    let error = errors.iter().find(|error| error["entityId"] == json!(refused)).unwrap_or(&Value::Null);
    assert_eq!(error["error"]["type"], json!(BAD_REQUEST_DATA), "batch create: {result}");
    assert!(
        !errors.iter().any(|error| error["entityId"] == json!(admitted)),
        "the sibling carrying a scope path is created: {result}"
    );
    assert_eq!(stored_scope(client, &admitted).await, Some(json!(MULTI_LEVEL_SCOPE)));

    scope.finish().await.expect("cleanup failed");
}

/// The two bounds a scope has, and their units.
///
/// A scope level is bounded in characters because that is the unit PostgreSQL
/// bounds an `ltree` label in: a level of 1000 two-byte letters weighs 2000
/// bytes and casts unrefused, verified against the deployed PostgreSQL 18.3,
/// which answers 1001 one-byte letters with "label string is too long. DETAIL:
/// Label length is 1001, must be at most 1000". The whole path is bounded in
/// bytes for a different reason: the `GiST` index row a label-path index builds
/// for one path must fit the 8191 bytes an index page admits, and that is
/// reached long before the level bound is. Both are refused as bad input --
/// clause 5.5.6 reserves `InternalError` for what the broker cannot decide
/// before it writes.
#[tokio::test]
async fn regression_scope_null_sentinel_a_scope_path_beyond_the_storable_bound_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let widest_level = "\u{00fc}".repeat(1000);
    let widest = format!("/{widest_level}");
    let stored = scope.id("Building:widest-scope-level");
    create_building(client, &stored, &json!(&widest)).await;
    assert_eq!(
        stored_scope(client, &stored).await,
        Some(json!(&widest)),
        "a level of 1000 two-byte letters is within the character bound the index counts"
    );

    let oversized_path = format!("/{widest_level}/{widest_level}");
    let refused = scope.id("Building:oversized-scope-path");
    let response = client
        .post_entity(&scoped_building(&refused, &json!(oversized_path)))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_bad_request_data(&response, "a path no index row can hold is bad input on create");

    scope.finish().await.expect("cleanup failed");
}
