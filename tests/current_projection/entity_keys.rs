//! Narrow Entity keys: child rows reference a compact surrogate, not the URI.
//!
//! `entities.entity_key` is the internal identity every owned child row
//! carries, while `entities.id` remains the external NGSI-LD URI clause 5.6.5
//! addresses. Identifiers up to the identifier bound must survive
//! round-tripping byte for byte, one past it must be refused as bad input, every
//! projection row must reference its own parent, current deletion must not
//! touch the independent temporal parent, and the surrogate must actually be
//! narrower than the identifier it replaced.

use crate::{
    bad_request::assert_bad_request_data,
    bounded_identifier::high_entropy_identifier,
    projection_rows::{entity_key, rows_for_key, temporal_entity_exists},
};
use andromeda_ngsi_ld_lexicon::identifier_length::IDENTIFIER_BYTE_LIMIT;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Map, Value, json};

/// Identifier lengths the surrogate has to carry: a short URN, a long one, and
/// one at the identifier bound itself, the longest an Entity id may be. The
/// short case is 128 rather than 64 bytes because the harness prefix that
/// makes a test's rows discoverable for cleanup is itself longer than 64
/// bytes.
const IDENTIFIER_LENGTHS: [usize; 3] = [128, 512, IDENTIFIER_BYTE_LIMIT];

/// Attribute counts paired with those lengths.
const ATTRIBUTE_COUNTS: [usize; 3] = [1, 10, 100];

/// Pad an identifier with an ASCII tail until it reaches the requested length.
fn identifier_of_length(base: &str, length: usize) -> String {
    let mut id = base.to_owned();
    while id.len() < length {
        id.push('a');
    }
    id
}

/// Build an Entity carrying `count` distinct numeric Properties.
fn building_with_attributes(id: &str, count: usize) -> Value {
    let mut object = Map::new();
    object.insert("id".to_owned(), Value::String(id.to_owned()));
    object.insert("type".to_owned(), Value::String("Building".to_owned()));
    for index in 0..count {
        object.insert(format!("attr{index}"), json!({"type": "Property", "value": index}));
    }
    object.insert("@context".to_owned(), json!([TEST_SUITE_CONTEXT]));
    Value::Object(object)
}

#[tokio::test]
async fn narrow_entity_keys_preserve_long_identifiers_and_own_their_projection_rows() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();

    for (length, attributes) in IDENTIFIER_LENGTHS.into_iter().zip(ATTRIBUTE_COUNTS) {
        let id = identifier_of_length(&scope.id(&format!("Building.width{length}.")), length);
        assert_eq!(id.len(), length, "the fixture identifier must have the intended byte length");

        let created = client
            .post_entity(&building_with_attributes(&id, attributes))
            .send()
            .await
            .expect("create entity failed");
        assert_eq!(created.status, StatusCode::CREATED, "create {length}-byte identifier: {:?}", created.body);

        let retrieved = client
            .get_entity(&id, "", "application/json", Some(TEST_SUITE_CONTEXT))
            .await
            .expect("retrieve entity failed");
        assert_eq!(retrieved.status, StatusCode::OK);
        let body = retrieved.json_value().expect("retrieval body must be JSON");
        assert_eq!(body["id"], Value::String(id.clone()), "the external identifier must round-trip exactly");

        let key = entity_key(scope.database_pool(), &id).await.expect("the Entity must have a surrogate key");
        assert_eq!(
            rows_for_key(scope.database_pool(), key).await,
            i64::try_from(attributes).expect("attribute counts are small"),
            "every attribute must be projected under its own parent key"
        );
        let foreign_parents: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM current_attributes AS c \
             WHERE c.entity_key = $1 AND NOT EXISTS (SELECT 1 FROM entities AS e WHERE e.entity_key = c.entity_key AND e.id = $2)",
        )
        .bind(key)
        .bind(&id)
        .fetch_one(scope.database_pool())
        .await
        .expect("parent check must be readable");
        assert_eq!(foreign_parents, 0, "no projection row may point at another Entity");
    }

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn narrow_entity_keys_keep_current_deletion_from_removing_the_temporal_parent() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:independent-temporal-parent");

    let created = client
        .post_entity(&building_with_attributes(&id, 3))
        .send()
        .await
        .expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
    assert!(
        temporal_entity_exists(scope.database_pool(), &id).await,
        "the write must have recorded an independent temporal Entity"
    );

    let deleted = client.delete_entity(&id).await.expect("delete entity failed");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete: {:?}", deleted.body);

    // Clause 5.6.6 deletes current state only; the temporal catalogue has its
    // own narrow key and no cascade edge from `entities`.
    assert_eq!(entity_key(scope.database_pool(), &id).await, None);
    assert!(
        temporal_entity_exists(scope.database_pool(), &id).await,
        "Core Entity deletion must leave the separate temporal parent standing"
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn narrow_entity_keys_cost_fewer_bytes_than_the_identifier_they_replace() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = identifier_of_length(&scope.id("Building.keywidth."), 512);

    let created = client
        .post_entity(&building_with_attributes(&id, 100))
        .send()
        .await
        .expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    // The multiplicity at which the surrogate wins: one parent identifier of
    // 512 bytes against a hundred child rows that would each have repeated it.
    let (surrogate_bytes, identifier_bytes): (i64, i64) = sqlx::query_as(
        "SELECT sum(pg_column_size(c.entity_key))::bigint, sum(pg_column_size(e.id))::bigint \
         FROM current_attributes AS c JOIN entities AS e ON e.entity_key = c.entity_key \
         WHERE e.id = $1",
    )
    .bind(&id)
    .fetch_one(scope.database_pool())
    .await
    .expect("column-size measurement must be readable");
    assert!(
        surrogate_bytes * 8 < identifier_bytes,
        "one hundred surrogate references ({surrogate_bytes} bytes) must be far cheaper than repeating the identifier ({identifier_bytes} bytes)"
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn narrow_entity_keys_store_a_high_entropy_identifier_at_the_limit() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = high_entropy_identifier(&scope.id("Building:entropy:"), IDENTIFIER_BYTE_LIMIT, 1);

    let created = client
        .post_entity(&building_with_attributes(&id, 1))
        .send()
        .await
        .expect("create entity failed");
    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "an identifier at the bound must be admitted: {:?}",
        created.body
    );

    // An incompressible identifier at the bound is the widest key every
    // identifier-keyed index has to hold; each parent must carry it verbatim.
    let key = entity_key(scope.database_pool(), &id)
        .await
        .expect("the Entity must be stored under its full identifier");
    assert!(
        temporal_entity_exists(scope.database_pool(), &id).await,
        "the temporal parent must be stored under the same full identifier"
    );
    assert_eq!(
        rows_for_key(scope.database_pool(), key).await,
        1,
        "the attribute must be projected under the Entity's surrogate"
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn narrow_entity_keys_refuse_an_identifier_one_byte_past_the_limit() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = high_entropy_identifier(&scope.id("Building:entropy-past:"), IDENTIFIER_BYTE_LIMIT + 1, 2);

    let refused = client
        .post_entity(&building_with_attributes(&id, 1))
        .send()
        .await
        .expect("create entity request failed");
    assert_bad_request_data(&refused, "creating an Entity whose id is one octet past the bound");
    assert_eq!(entity_key(scope.database_pool(), &id).await, None, "the refused Entity must not be stored");

    scope.finish().await.expect("cleanup failed");
}
