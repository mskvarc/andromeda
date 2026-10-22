//! Typed `q` predicates: pushed-down comparisons agree with the stored value.
//!
//! One Entity carries a fixture of every value shape a row holds in a typed
//! column, plus the shapes held in `value_object` or among the sub-attributes.
//! Every predicate is compared against the residual oracle (the value the
//! fixture actually holds), and every shape has to give the same answer.

use andromeda_postgresql_schema::{
    current::{CurrentAttributes, Entities},
    name_dictionary::AttributeNames,
};
use andromeda_postgresql_seaquery::{aggregate::count_all, statement::Statement};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::{context::TEST_SUITE_CONTEXT, high_entropy_text::high_entropy_text};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use sea_query::{Expr, ExprTrait, Query, SelectStatement};
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

/// Characters of high-entropy text behind the leading `m` of the long-string
/// fixture: far past the 2048-octet bound, so the value is held in the
/// overflow column `value_long_text` rather than in `value_string`.
const ESSAY_TAIL_LENGTH: usize = 6000;

/// Seed of the long-string fixture text, fixed so every run stores the same
/// value.
const ESSAY_SEED: u64 = 4_009;

/// Build the long-string fixture Entity: one String past the indexed bound and
/// one short String beside it.
fn long_string_fixture(id: &str, essay: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "essay": {"type": "Property", "value": essay},
        "note": {"type": "Property", "value": "short"},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Run one `q` filter through the POST query form (clause 5.7.2) restricted to
/// the fixture Entity and report whether it selected it. The filter travels in
/// the body, so an operand far longer than any request line still reaches the
/// broker.
async fn posted_query_selects_fixture(client: &TestClient, id: &str, label: &str, filter: &str) -> bool {
    let body = json!({
        "type": "Query",
        "entities": [{"id": id, "type": "Building"}],
        "q": filter,
        "@context": TEST_SUITE_CONTEXT
    });
    let response = client.batch_query(&body, "application/json", None).await.expect("query failed");
    assert_eq!(response.status, StatusCode::OK, "{label}: {:?}", response.body);
    let body = response.json_value().expect("query body must be JSON");
    body.as_array().is_some_and(|entities| !entities.is_empty())
}

/// Read the migration-owned kind codes of one attribute of one Entity.
async fn kinds_of(pool: &PgPool, id: &str, local_name: &str) -> (i16, i16) {
    projected_attribute(id, local_name)
        .columns([
            (CurrentAttributes::Table, CurrentAttributes::AttributeKind),
            (CurrentAttributes::Table, CurrentAttributes::ValueKind),
        ])
        .to_query_as()
        .fetch_one(pool)
        .await
        .expect("the attribute must be projected")
}

/// The projection rows of the Entity `id` whose attribute IRI ends in
/// `local_name`, before any column is selected.
fn projected_attribute(id: &str, local_name: &str) -> SelectStatement {
    Query::select()
        .from(CurrentAttributes::Table)
        .inner_join(
            Entities::Table,
            Expr::col((Entities::Table, Entities::EntityKey)).equals((CurrentAttributes::Table, CurrentAttributes::EntityKey)),
        )
        .inner_join(
            AttributeNames::Table,
            Expr::col((AttributeNames::Table, AttributeNames::AttributeNameKey)).equals((CurrentAttributes::Table, CurrentAttributes::AttributeNameKey)),
        )
        .and_where(Expr::col((Entities::Table, Entities::Id)).eq(id))
        .and_where(Expr::col((AttributeNames::Table, AttributeNames::Name)).like(format!("%{local_name}")))
        .to_owned()
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

/// A String past the 2048-octet bound is projected into `value_long_text`, and
/// every clause 4.9 operator over it must still agree with the value the
/// fixture holds: equality reads the one column the operand itself would be
/// placed in, ordering and patterns read both string columns, and a long
/// operand compared against a short value selects nothing.
#[tokio::test]
async fn typed_q_predicates_agree_with_the_residual_oracle_for_long_strings() {
    let scope = TestEnvironment::from_suite("query_access_paths").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:typed-q-long-strings");
    let essay = format!("m{}", high_entropy_text(ESSAY_TAIL_LENGTH, ESSAY_SEED));
    let created = client
        .post_entity(&long_string_fixture(&id, &essay))
        .send()
        .await
        .expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let truncated = essay.get(..essay.len() - 1).expect("the essay is ASCII, so every index is a boundary");
    let middle = essay
        .get(ESSAY_TAIL_LENGTH / 2..ESSAY_TAIL_LENGTH / 2 + 16)
        .expect("the essay is ASCII, so every index is a boundary");

    // (label, filter, whether the fixture satisfies it). The label keeps a
    // failure message readable where the filter itself is six kilobytes long.
    let cases = [
        ("essay equals itself", format!("essay==\"{essay}\""), true),
        ("essay equals its own prefix", format!("essay==\"{truncated}\""), false),
        ("essay differs from itself", format!("essay!=\"{essay}\""), false),
        ("essay differs from a short string", "essay!=\"short\"".to_owned(), true),
        ("essay orders after a", "essay>\"a\"".to_owned(), true),
        ("essay orders before a", "essay<\"a\"".to_owned(), false),
        ("essay orders before z", "essay<\"z\"".to_owned(), true),
        ("essay lies in the range a to z", "essay==\"a\"..\"z\"".to_owned(), true),
        ("essay matches its own middle", format!("essay~=\"{middle}\""), true),
        ("essay matches an absent anchored prefix", "essay~=\"^zzz\"".to_owned(), false),
        ("essay does not match its own middle", format!("essay!~=\"{middle}\""), false),
        ("essay does not match an absent anchored prefix", "essay!~=\"^zzz\"".to_owned(), true),
        ("essay exists", "essay".to_owned(), true),
        ("short note equals the essay", format!("note==\"{essay}\""), false),
    ];
    for (label, filter, expected) in cases {
        assert_eq!(posted_query_selects_fixture(client, &id, label, &filter).await, expected, "{label}");
    }

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn typed_q_predicates_beyond_double_precision_compare_exactly() {
    let scope = TestEnvironment::from_suite("query_access_paths").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:typed-q-precision");
    let created = client.post_entity(&typed_fixture(&id)).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    // `value_number` holds the JSON number exactly and the operand binds as an
    // exact decimal, so a neighbouring double is a different number.
    assert!(
        selects_fixture(client, &id, &format!("counter=={BEYOND_DOUBLE}")).await,
        "the exact value must match"
    );
    assert!(
        !selects_fixture(client, &id, &format!("counter=={}", BEYOND_DOUBLE - 1)).await,
        "the neighbouring double must not match"
    );

    // A sub-attribute predicate reads the element's number, also exactly.
    assert!(selects_fixture(client, &id, "precise.accuracy==0.5").await);
    assert!(!selects_fixture(client, &id, "precise.accuracy==0.6").await);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn typed_q_predicates_read_compound_values_from_the_value_object() {
    let scope = TestEnvironment::from_suite("query_access_paths").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:typed-q-compound");
    let created = client.post_entity(&typed_fixture(&id)).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    // A compound value is one `value_object`, so a predicate addressing a
    // member inside it reads that member (clause 4.9 compound value path).
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

    let tombstones: i64 = Query::select()
        .expr(count_all())
        .from(CurrentAttributes::Table)
        .inner_join(
            Entities::Table,
            Expr::col((Entities::Table, Entities::EntityKey)).equals((CurrentAttributes::Table, CurrentAttributes::EntityKey)),
        )
        .and_where(Expr::col((Entities::Table, Entities::Id)).eq(id.as_str()))
        .and_where(Expr::col((CurrentAttributes::Table, CurrentAttributes::ValueKind)).eq(0))
        .to_query_scalar()
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
    let projected: Value = projected_attribute(&id, "members")
        .column((CurrentAttributes::Table, CurrentAttributes::ValueObject))
        .to_query_scalar()
        .fetch_one(scope.database_pool())
        .await
        .expect("the ListRelationship must be projected");
    assert!(
        projected.to_string().contains(RELATED),
        "the projected object list must name its targets: {projected}"
    );

    scope.finish().await.expect("cleanup failed");
}
