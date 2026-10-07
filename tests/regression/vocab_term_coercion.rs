//! A `VocabProperty` `vocab` is any string the `@context` coerces to a URI.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.2.35, Table 5.2.35-1: `vocab` holds "String
//! Values which shall be type coerced to URIs based on the supplied @context",
//! with no restriction on the string. Annex C clause C.2.2 writes the
//! `category` of a Vehicle as `"vocab": "non-commercial"`, a term whose `-` the
//! clause 4.6.2 short-name grammar does not admit. Reading the compact body
//! typed before expansion must not hold `vocab` to that grammar and refuse the
//! entity with `400 BadRequestData`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

/// The IRI the user `@context` gives the term `non-commercial`.
const NON_COMMERCIAL: &str = "https://example.org/vocab/non-commercial";

/// Clause 5.2.35: a hyphenated term and an array of terms are accepted as
/// `vocab`, and the entity reads back with each expanded to the IRI the user
/// `@context` defines.
#[tokio::test]
async fn regression_vocab_term_a_hyphenated_vocab_term_is_created_and_read_back() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:vocab-term");
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "category": {"type": "VocabProperty", "vocab": "non-commercial"},
        "categories": {"type": "VocabProperty", "vocab": ["non-commercial", "commercial"]},
        "@context": [CORE_CONTEXT, {"non-commercial": NON_COMMERCIAL, "commercial": "https://example.org/vocab/commercial"}]
    });
    let created = client.post_entity(&entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let retrieved = client.get_entity(&id, "", "application/ld+json", None).await.expect("retrieve entity request");
    scope.finish().await.expect("environment cleanup");

    assert_eq!(retrieved.status, StatusCode::OK, "{:?}", retrieved.body);
    let body = retrieved.json_value().expect("an entity is JSON");
    assert_eq!(body["category"]["type"], json!("VocabProperty"), "{body}");
    assert_eq!(body["category"]["vocab"], json!(NON_COMMERCIAL), "{body}");
    assert_eq!(
        body["categories"]["vocab"],
        json!([NON_COMMERCIAL, "https://example.org/vocab/commercial"]),
        "{body}"
    );
}
