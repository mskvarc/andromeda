//! A registered `idPattern` (Table 5.2.8-1), evaluated by the store when
//! discovery asks which registrations cover the identifiers it names (clause
//! 5.12) and when a request is forwarded to the registrations covering an
//! Entity (clause 5.10.2).
//!
//! The store reads a registered pattern out of the registration's information
//! rows inside the statement that decides, so each row persists its pattern's
//! restatement beside it and the statement evaluates that. The identifiers here
//! are ASCII and the patterns fold through a scoped group: PostgreSQL refuses a
//! flag group anywhere but at the start of a pattern, so a statement that
//! evaluated the registered text would fail outright rather than disagree.

use andromeda_ngsi_ld_query::{pattern::Pattern, portable_pattern::PortablePattern};
use andromeda_postgresql_context_store::csource_registration::pattern_forms_refresh::refresh_stale_pattern_forms;
use andromeda_test_client::response::TestResponse;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{callback_path::CallbackPath, context_source_server::ContextSourceServer, server_config::MockServerConfig};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Map, Value, json};
use std::{collections::BTreeSet, time::Duration};

/// An endpoint no scenario but the forwarding one ever reaches.
const UNREACHED_ENDPOINT: &str = "http://csource.invalid:1026";

/// `^<run prefix>:(?i:<word>):`, which folds `word` and nothing else.
fn folded_segment_pattern(scope: &TestEnvironment, word: &str) -> String {
    format!("^{}:(?i:{word}):", regex::escape(scope.id_prefix()))
}

/// Create a registration of the Entities one `EntityInfo` selects.
async fn register(scope: &TestEnvironment, local_id: &str, entity_info: Value, endpoint: &str) -> String {
    let id = scope.id(local_id);
    let registration = json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [entity_info]}],
        "endpoint": endpoint,
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = scope.client().create_registration(&registration).await.expect("create registration request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    id
}

/// The registrations of `entity_type` discovery answers `criteria` with.
async fn discovered(scope: &TestEnvironment, entity_type: &str, criteria: &str) -> BTreeSet<String> {
    let response: TestResponse = scope
        .client()
        .query_registrations(&format!("type={entity_type}&{criteria}"), Some(TEST_SUITE_CONTEXT))
        .await
        .expect("registration query request");
    assert_eq!(response.status, StatusCode::OK, "{criteria}: {:?}", response.body);
    response
        .json_value()
        .expect("discovery answers with JSON")
        .as_array()
        .expect("discovery answers with an array")
        .iter()
        .map(|registration| registration["id"].as_str().expect("a registration carries an id").to_owned())
        .collect()
}

/// `id=<identifier>`, percent-encoded.
fn identifier_criteria(identifier: &str) -> String {
    format!("id={}", urlencoding::encode(identifier))
}

/// The restatements the registration's information rows persisted, keyed by
/// the pattern as written.
async fn stored_forms(scope: &TestEnvironment, registration: &str) -> Option<Value> {
    sqlx::query_scalar::<_, Option<Value>>(
        "SELECT jsonb_object_agg(entity_id_pattern, entity_id_pattern_form) FROM registration_information \
         WHERE registration_id = $1 AND entity_id_pattern IS NOT NULL",
    )
    .bind(registration)
    .fetch_one(scope.database_pool())
    .await
    .expect("the registration row exists")
}

/// The stored restatements of a registration declaring `pattern` alone.
fn stored_member(pattern: &str) -> Value {
    let parsed = Pattern::parse(pattern).expect("the pattern is admitted");
    Value::Object(Map::from_iter([(pattern.to_owned(), Value::String(parsed.portable().as_str().to_owned()))]))
}

/// The registration stores, beside the pattern as written, the restatement
/// the store evaluates; the representation clients read keeps the text.
#[tokio::test]
async fn unicode_pattern_registration_stores_each_pattern_beside_its_restatement() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let entity_type = scope.entity_type("StoredHive");
    let pattern = folded_segment_pattern(&scope, "hive");
    let registration = register(
        &scope,
        "Registration:stored",
        json!({"type": entity_type, "idPattern": pattern}),
        UNREACHED_ENDPOINT,
    )
    .await;

    let forms = stored_forms(&scope, &registration).await;
    assert_eq!(forms, Some(stored_member(&pattern)));
    let restatement = forms.as_ref().and_then(|forms| forms[pattern.as_str()].as_str()).unwrap_or_default();
    assert!(!restatement.contains("(?i"), "no option is left for PostgreSQL to read: {restatement}");

    let retrieved = scope
        .client()
        .retrieve_registration(&registration, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve registration request");
    let body = retrieved.json_value().expect("a registration is JSON");
    assert_eq!(body["information"][0]["entities"][0]["idPattern"], json!(pattern), "{body}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.12, third condition: an identifier discovery names matches the
/// registered pattern, which folds the enclosed segment only.
#[tokio::test]
async fn unicode_pattern_registration_discovery_by_identifier_evaluates_the_registered_pattern() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let entity_type = scope.entity_type("DiscoveredHive");
    let pattern = folded_segment_pattern(&scope, "hive");
    let registration = register(
        &scope,
        "Registration:by-identifier",
        json!({"type": entity_type, "idPattern": pattern}),
        UNREACHED_ENDPOINT,
    )
    .await;

    for (local, covered) in [("HIVE:1", true), ("Hive:2", true), ("hive:3", true), ("Hivex:4", false)] {
        let found = discovered(&scope, &entity_type, &identifier_criteria(&scope.id(local))).await;
        assert_eq!(found.contains(&registration), covered, "{local}: {found:?}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// A registered pattern built from shorthand classes is stored as their
/// Unicode ranges and evaluated as such: `\w+` covers one segment of word
/// characters, `\d+` one of digits, and neither covers a hyphen.
#[tokio::test]
async fn unicode_pattern_registration_a_shorthand_class_pattern_is_stored_as_its_ranges_and_evaluated() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let entity_type = scope.entity_type("ClassedHive");
    let pattern = format!(r"^{}:\w+:\d+$", regex::escape(scope.id_prefix()));
    let registration = register(
        &scope,
        "Registration:classed",
        json!({"type": entity_type, "idPattern": pattern}),
        UNREACHED_ENDPOINT,
    )
    .await;

    let forms = stored_forms(&scope, &registration).await;
    assert_eq!(forms, Some(stored_member(&pattern)));
    let restatement = forms.as_ref().and_then(|forms| forms[pattern.as_str()].as_str()).unwrap_or_default();
    assert!(
        !restatement.contains(r"\w") && !restatement.contains(r"\d"),
        "no shorthand is left for PostgreSQL to read: {restatement}"
    );

    for (local, covered) in [("Hive_1:12", true), ("Hive:x", false), ("Hi-ve:1", false)] {
        let found = discovered(&scope, &entity_type, &identifier_criteria(&scope.id(local))).await;
        assert_eq!(found.contains(&registration), covered, "{local}: {found:?}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.12, fourth condition: the pattern discovery names matches a
/// registered identifier; the queried pattern is evaluated as its restatement
/// too.
#[tokio::test]
async fn unicode_pattern_registration_discovery_by_pattern_matches_a_registered_identifier() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let entity_type = scope.entity_type("PatternHive");
    let registration = register(
        &scope,
        "Registration:by-pattern",
        json!({"type": entity_type, "id": scope.id("HIVE:7")}),
        UNREACHED_ENDPOINT,
    )
    .await;

    let criteria = format!("idPattern={}", urlencoding::encode(&folded_segment_pattern(&scope, "hive")));
    assert_eq!(discovered(&scope, &entity_type, &criteria).await, BTreeSet::from([registration]));
    let criteria = format!("idPattern={}", urlencoding::encode(&folded_segment_pattern(&scope, "wasp")));
    assert_eq!(discovered(&scope, &entity_type, &criteria).await, BTreeSet::new());

    scope.finish().await.expect("cleanup failed");
}

/// Replacing the `information` replaces the stored restatements with it: the
/// old pattern no longer covers anything and the new one does.
#[tokio::test]
async fn unicode_pattern_registration_an_update_rewrites_the_stored_restatements() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let entity_type = scope.entity_type("UpdatedHive");
    let hive = folded_segment_pattern(&scope, "hive");
    let wasp = folded_segment_pattern(&scope, "wasp");
    let registration = register(
        &scope,
        "Registration:updated",
        json!({"type": entity_type, "idPattern": hive}),
        UNREACHED_ENDPOINT,
    )
    .await;

    let fragment = json!({"information": [{"entities": [{"type": entity_type, "idPattern": wasp}]}]});
    let updated = scope
        .client()
        .update_registration(&registration, &fragment, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("update registration request");
    assert_eq!(updated.status, StatusCode::NO_CONTENT, "{:?}", updated.body);

    assert_eq!(stored_forms(&scope, &registration).await, Some(stored_member(&wasp)));
    let by_wasp = discovered(&scope, &entity_type, &identifier_criteria(&scope.id("WASP:1"))).await;
    assert!(by_wasp.contains(&registration), "{by_wasp:?}");
    let by_hive = discovered(&scope, &entity_type, &identifier_criteria(&scope.id("HIVE:1"))).await;
    assert!(!by_hive.contains(&registration), "{by_hive:?}");

    scope.finish().await.expect("cleanup failed");
}

/// A restatement an older rendering stored is what discovery evaluates until
/// the tenant is opened again, and the refresh a broker runs on opening renders
/// it again from the registration's `information`.
#[tokio::test]
async fn unicode_pattern_registration_a_restatement_an_older_rendering_stored_is_rendered_again() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let entity_type = scope.entity_type("RefreshedHive");
    let pattern = folded_segment_pattern(&scope, "hive");
    let registration = register(
        &scope,
        "Registration:refreshed",
        json!({"type": entity_type, "idPattern": pattern}),
        UNREACHED_ENDPOINT,
    )
    .await;
    let covered = identifier_criteria(&scope.id("HIVE:1"));

    sqlx::query("UPDATE registration_information SET entity_id_pattern_form = '^never$' WHERE registration_id = $1")
        .bind(&registration)
        .execute(scope.database_pool())
        .await
        .expect("the rows are set back to an older rendering");
    sqlx::query("UPDATE context_source_registrations SET entity_id_pattern_forms_revision = 0 WHERE id = $1")
        .bind(&registration)
        .execute(scope.database_pool())
        .await
        .expect("the registration is set back to an older revision");
    assert!(
        !discovered(&scope, &entity_type, &covered).await.contains(&registration),
        "discovery evaluates the stored restatement, not the registered text"
    );

    let rewritten = refresh_stale_pattern_forms(scope.database_pool()).await.expect("the refresh runs");
    assert!(rewritten >= 1, "the outdated row is rewritten");
    assert_eq!(stored_forms(&scope, &registration).await, Some(stored_member(&pattern)));
    let revision: Option<i16> = sqlx::query_scalar("SELECT entity_id_pattern_forms_revision FROM context_source_registrations WHERE id = $1")
        .bind(&registration)
        .fetch_one(scope.database_pool())
        .await
        .expect("the registration row exists");
    assert_eq!(revision, Some(PortablePattern::RENDERING_REVISION.get()));
    assert!(discovered(&scope, &entity_type, &covered).await.contains(&registration));

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.10.2: a retrieval is forwarded to the registration whose pattern
/// covers the identifier, and to no registration whose pattern does not.
#[tokio::test]
async fn unicode_pattern_registration_forwarding_reaches_the_source_whose_pattern_covers_the_identifier() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let source = ContextSourceServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("context source must start");
    let root = CallbackPath::parse("/").expect("absolute source root");
    let endpoint = source.endpoint(&root).expect("source endpoint").to_string();
    let entity_type = scope.entity_type("ForwardedHive");
    register(
        &scope,
        "Registration:forwarded",
        json!({"type": entity_type, "idPattern": folded_segment_pattern(&scope, "hive")}),
        &endpoint,
    )
    .await;

    let covered = scope.id("HIVE:9");
    let response = scope
        .client()
        .get_entity(&covered, "", "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve entity request");
    let forwarded = source
        .next_request(Duration::from_secs(5))
        .await
        .map_err(|error| format!("{covered}: {error:?}; broker answered {:?}", response.body))
        .expect("the covering source is asked for the Entity");
    assert!(forwarded.uri.path().contains("HIVE"), "{}", forwarded.uri);

    let uncovered = scope.id("Hivex:9");
    scope
        .client()
        .get_entity(&uncovered, "", "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve entity request");
    assert_eq!(source.total_request_count(), 1, "no source covers {uncovered}");

    source.shutdown().await.expect("context source shutdown failed");
    scope.finish().await.expect("cleanup failed");
}
