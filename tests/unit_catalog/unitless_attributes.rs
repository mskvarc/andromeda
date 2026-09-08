//! Verification matrix row "Unitless attributes".
//!
//! Clauses 4.5.3 (Relationship), 4.5.18 (`LanguageProperty`), 4.5.20
//! (`VocabProperty`) and 4.5.24 (`JsonProperty`) define those attribute families
//! without a `unitCode` member: a relationship target and a language map are
//! not quantities, so a unit on them means nothing the broker could store or
//! render back. Every such body is refused, and nothing about the refused write
//! reaches persistence.

use crate::fixtures::{SUITE, default_context_iri, entity_with_unitless_attribute, history_unit_identities};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The four families the specification defines without a unit, each carrying
/// one, with the clause that makes the shape invalid.
fn unitless_fixtures() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        (
            "relationship",
            "4.5.3",
            json!({"type": "Relationship", "object": "urn:ngsi-ld:Device:target", "unitCode": "KGM"}),
        ),
        (
            "language-property",
            "4.5.18",
            json!({"type": "LanguageProperty", "languageMap": {"en": "label"}, "unitCode": "KGM"}),
        ),
        ("vocab-property", "4.5.20", json!({"type": "VocabProperty", "vocab": "term", "unitCode": "KGM"})),
        (
            "json-property",
            "4.5.24",
            json!({"type": "JsonProperty", "json": {"member": 1}, "unitCode": "KGM"}),
        ),
    ]
}

#[tokio::test]
async fn an_attribute_family_the_specification_leaves_unitless_cannot_carry_a_unit() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();

    for (label, clause, attribute) in unitless_fixtures() {
        let id = scope.id(&format!("Device:unitless-{label}"));
        let created = client
            .post_entity(&entity_with_unitless_attribute(&id, attribute))
            .send()
            .await
            .expect("create entity request failed");
        assert_ne!(
            created.status,
            StatusCode::CREATED,
            "clause {clause} leaves this family unitless, so the write must be refused: {:?}",
            created.body
        );

        let retrieved = client
            .get_entity(&id, "", "application/json", None)
            .await
            .expect("retrieve entity request failed");
        assert_eq!(
            retrieved.status,
            StatusCode::NOT_FOUND,
            "a refused {label} write must leave no entity behind: {:?}",
            retrieved.body
        );
        assert!(
            history_unit_identities(scope.database_pool(), &id, &default_context_iri("member"))
                .await
                .is_empty(),
            "a refused {label} write must leave no history row behind"
        );
    }

    scope.finish().await.expect("cleanup failed");
}

/// The shape is invalid input, so clause 5.5.9 makes the refusal
/// `BadRequestData` rather than an internal error the client could retry.
#[tokio::test]
async fn a_unit_on_a_unitless_family_is_refused_as_bad_request_data() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();

    for (label, clause, attribute) in unitless_fixtures() {
        let id = scope.id(&format!("Device:unitless-status-{label}"));
        let created = client
            .post_entity(&entity_with_unitless_attribute(&id, attribute))
            .send()
            .await
            .expect("create entity request failed");
        assert_eq!(
            created.status,
            StatusCode::BAD_REQUEST,
            "a {label} carrying a unit violates clause {clause} and is invalid input: {:?}",
            created.body
        );
    }

    scope.finish().await.expect("cleanup failed");
}
