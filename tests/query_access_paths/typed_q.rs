//! Verification-matrix row "Typed q predicates".
//!
//! One Entity carries a fixture of every value shape the projection can hold,
//! plus the shapes it cannot. Every predicate is compared against the residual
//! oracle — the value the fixture actually holds — and the shapes that cannot be
//! pushed down have to fall back to the payload without changing the answer.

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;

/// The Relationship target of the fixture.
const RELATED: &str = "urn:ngsi-ld:query-access-paths:Related:1";

/// A Property value beyond the exactly representable `double precision` range.
const BEYOND_DOUBLE: i64 = 9_007_199_254_740_993;

/// Build the fixture Entity carrying one instance of every value shape.
fn typed_fixture(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "reading": {"type": "Property", "value": 21.5},
        "category": {"type": "Property", "value": "alpha"},
        "active": {"type": "Property", "value": true},
        "seen": {"type": "Property", "value": {"@type": "DateTime", "@value": "2024-05-01T00:00:00Z"}},
        "manual": {"type": "Property", "value": "https://example.test/manual"},
        "address": {"type": "Property", "value": {"city": "Berlin"}},
        "tags": {"type": "ListProperty", "valueList": [1, 2, 3]},
        "name": {"type": "LanguageProperty", "languageMap": {"en": "house", "de": "Haus"}},
        "link": {"type": "Relationship", "object": RELATED},
        "members": {"type": "ListRelationship", "objectList": [RELATED]},
        "counter": {"type": "Property", "value": BEYOND_DOUBLE},
        "precise": {"type": "Property", "value": 7, "accuracy": {"type": "Property", "value": 0.5}},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Run one `q` filter restricted to the fixture Entity and report whether it
/// selected it.
async fn selects_fixture(client: &TestClient, id: &str, filter: &str) -> bool {
    let query = format!("type=Building&id={}&q={}", urlencoding::encode(id), urlencoding::encode(filter));
    let response = client
        .query_entities(&query, "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query failed");
    assert_eq!(response.status, StatusCode::OK, "q={filter}: {:?}", response.body);
    let body = response.json_value().expect("query body must be JSON");
    body.as_array().is_some_and(|entities| !entities.is_empty())
}

/// Read the migration-owned kind codes of one attribute of one Entity.
async fn kinds_of(pool: &PgPool, id: &str, local_name: &str) -> (i16, i16) {
    sqlx::query_as(
        "SELECT c.attribute_kind, c.value_kind FROM current_attribute_values AS c \
         JOIN entities AS e ON e.entity_key = c.entity_key \
         JOIN attribute_names AS an ON an.id = c.attribute_name_id \
         WHERE e.id = $1 AND an.name LIKE '%' || $2",
    )
    .bind(id)
    .bind(local_name)
    .fetch_one(pool)
    .await
    .expect("the attribute must be projected")
}

#[tokio::test]
async fn typed_q_predicates_agree_with_the_residual_oracle_for_every_scalar_shape() {
    let scope = TestEnvironment::from_suite("query_access_paths").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:typed-q-scalars");
    let created = client.post_entity(&typed_fixture(&id)).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    // (filter, whether the fixture satisfies it)
    let cases = [
        ("reading==21.5", true),
        ("reading!=21.5", false),
        ("reading>21", true),
        ("reading<21", false),
        ("reading==21..22", true),
        ("reading==1..2", false),
        ("category==\"alpha\"", true),
        ("category==\"beta\"", false),
        ("category~=\"lph\"", true),
        ("category!~=\"lph\"", false),
        ("active==true", true),
        ("active==false", false),
        ("seen==\"2024-05-01T00:00:00Z\"", true),
        ("seen>\"2024-04-30T00:00:00Z\"", true),
        ("seen<\"2024-04-30T00:00:00Z\"", false),
        ("manual==\"https://example.test/manual\"", true),
        ("manual==\"https://example.test/other\"", false),
        ("link==\"urn:ngsi-ld:query-access-paths:Related:1\"", true),
        ("link==\"urn:ngsi-ld:query-access-paths:Related:2\"", false),
        ("reading", true),
        ("missing", false),
        ("tags", true),
        ("name", true),
        ("members", true),
    ];
    for (filter, expected) in cases {
        assert_eq!(selects_fixture(client, &id, filter).await, expected, "q={filter}");
    }

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn typed_q_predicates_beyond_double_precision_fall_back_without_changing_semantics() {
    let scope = TestEnvironment::from_suite("query_access_paths").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:typed-q-precision");
    let created = client.post_entity(&typed_fixture(&id)).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    // The operand cannot travel to the projection exactly, so the predicate
    // must be answered from the payload, which holds the JSON number verbatim.
    assert!(
        selects_fixture(client, &id, &format!("counter=={BEYOND_DOUBLE}")).await,
        "the exact value must match"
    );
    assert!(
        !selects_fixture(client, &id, &format!("counter=={}", BEYOND_DOUBLE - 1)).await,
        "the neighbouring double must not match, which is the whole reason for the fallback"
    );

    // A sub-attribute predicate is not projectable either, and must still be
    // answered exactly.
    assert!(selects_fixture(client, &id, "precise.accuracy==0.5").await);
    assert!(!selects_fixture(client, &id, "precise.accuracy==0.6").await);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn typed_q_predicates_read_compound_values_from_the_payload() {
    let scope = TestEnvironment::from_suite("query_access_paths").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:typed-q-compound");
    let created = client.post_entity(&typed_fixture(&id)).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    // The projection stores a compound value as one opaque JSONB object, so a
    // predicate addressing a member inside it is answered by the payload
    // fallback (clause 4.9 compound value path).
    assert!(selects_fixture(client, &id, "address[city]==\"Berlin\"").await);
    assert!(!selects_fixture(client, &id, "address[city]==\"Paris\"").await);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn typed_q_fixtures_are_projected_under_their_declared_kinds() {
    let scope = TestEnvironment::from_suite("query_access_paths").await.expect("isolated test environment");
    let client = scope.client();
    let pool = scope.database_pool();
    let id = scope.id("Building:typed-q-kinds");
    let created = client.post_entity(&typed_fixture(&id)).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    // Attribute kind 1 Property … 8 ListRelationship; value kind 1 Number,
    // 2 String, 3 Boolean, 5 Object, 6 DateTime, 7 Uri. Kind 0 is the
    // tombstone and must never reach current state.
    for (attribute, expected) in [
        ("reading", (1_i16, 1_i16)),
        ("category", (1, 2)),
        ("active", (1, 3)),
        ("seen", (1, 6)),
        ("link", (2, 7)),
        ("address", (1, 5)),
        ("tags", (7, 5)),
        ("name", (5, 5)),
        ("members", (8, 5)),
    ] {
        assert_eq!(kinds_of(pool, &id, attribute).await, expected, "kinds of {attribute}");
    }

    let tombstones: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM current_attribute_values AS c \
         JOIN entities AS e ON e.entity_key = c.entity_key WHERE e.id = $1 AND c.value_kind = 0",
    )
    .bind(&id)
    .fetch_one(pool)
    .await
    .expect("tombstone count must be readable");
    assert_eq!(tombstones, 0, "the reserved tombstone kind never appears in current state");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn typed_q_list_relationship_projects_the_related_entity_identifiers() {
    let scope = TestEnvironment::from_suite("query_access_paths").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:typed-q-list-relationship");
    let created = client.post_entity(&typed_fixture(&id)).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    // A ListRelationship targets Entities, so its projected object must carry
    // the target identifiers, not an empty list (clauses 4.5.4 and 4.5.24).
    let projected: Value = sqlx::query_scalar(
        "SELECT c.value_object FROM current_attribute_values AS c \
         JOIN entities AS e ON e.entity_key = c.entity_key \
         JOIN attribute_names AS an ON an.id = c.attribute_name_id \
         WHERE e.id = $1 AND an.name LIKE '%members'",
    )
    .bind(&id)
    .fetch_one(scope.database_pool())
    .await
    .expect("the ListRelationship must be projected");
    assert!(
        projected.to_string().contains(RELATED),
        "the projected object list must name its targets: {projected}"
    );

    scope.finish().await.expect("cleanup failed");
}
