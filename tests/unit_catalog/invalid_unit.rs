//! Invalid unit: a `unitCode` the catalogue does not register is refused.
//!
//! A `unitCode` the catalogue does not register has no identity to store, so
//! the write is refused rather than silently stripped: an attribute persisted
//! without the unit its client declared would mean something else. The
//! database holds the same line for a writer that bypasses the broker — the
//! current-state projection carries a foreign key to `unit_codes` — and no
//! history row may carry a unit only as leftover metadata.

use crate::fixtures::{EXPANDED_UNIT_CODE, SUITE, default_context_iri, device_with_unit, history_metadata, history_unit_identities};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

/// A three-letter token of the right shape that Recommendation 20 never assigned.
const UNREGISTERED_CODE: &str = "QQQ";

#[tokio::test]
async fn a_property_carrying_an_unregistered_unit_code_is_refused_and_persists_nothing() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:invalid-unit");

    let entity = json!({
        "id": id,
        "type": "Device",
        "weight": {"type": "Property", "value": 5, "unitCode": UNREGISTERED_CODE},
        "@context": [CORE_CONTEXT]
    });
    let created = client.post_entity(&entity).send().await.expect("create entity request failed");
    assert_ne!(
        created.status,
        StatusCode::CREATED,
        "a code the catalogue cannot register must not be accepted: {:?}",
        created.body
    );

    let retrieved = client
        .get_entity(&id, "", "application/json", None)
        .await
        .expect("retrieve entity request failed");
    assert_eq!(
        retrieved.status,
        StatusCode::NOT_FOUND,
        "a refused write must leave no entity behind: {:?}",
        retrieved.body
    );

    assert!(
        history_unit_identities(scope.database_pool(), &id, &default_context_iri("weight"))
            .await
            .is_empty(),
        "a refused write must leave no history row behind"
    );

    scope.finish().await.expect("cleanup failed");
}

/// The refusal is a statement about client input, so clause 5.5.9 makes it
/// `BadRequestData`; an `InternalError` would tell the client to retry a
/// request that can never succeed.
#[tokio::test]
async fn an_unregistered_unit_code_is_refused_as_bad_request_data() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:invalid-unit-status");

    let entity = json!({
        "id": id,
        "type": "Device",
        "weight": {"type": "Property", "value": 5, "unitCode": UNREGISTERED_CODE},
        "@context": [CORE_CONTEXT]
    });
    let created = client.post_entity(&entity).send().await.expect("create entity request failed");
    assert_eq!(
        created.status,
        StatusCode::BAD_REQUEST,
        "unusable client input is not an internal error: {:?}",
        created.body
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn a_bypass_write_of_an_unregistered_unit_identity_is_refused_by_the_foreign_key() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:bypass-unit");

    let created = client
        .post_entity(&device_with_unit(&id, "KGM", "KGM"))
        .send()
        .await
        .expect("create entity request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    // 32767 is the largest identity the two-byte key can hold and one the
    // catalogue has never assigned, so the projection's foreign key is the only
    // thing that can refuse it.
    let bypass = sqlx::query(
        "UPDATE current_attribute_values SET unit_code_id = 32767 \
         WHERE entity_key = (SELECT entity_key FROM entities WHERE id = $1)",
    )
    .bind(&id)
    .execute(scope.database_pool())
    .await;

    let refusal = bypass.expect_err("the projection must refuse a unit identity the catalogue does not hold");
    let code = refusal.as_database_error().and_then(|error| error.code().map(std::borrow::Cow::into_owned));
    assert_eq!(code.as_deref(), Some("23503"), "the refusal must be a foreign-key violation: {refusal}");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn no_history_row_carries_a_unit_only_as_leftover_metadata() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:metadata-unit");

    let created = client
        .post_entity(&device_with_unit(&id, "CEL", "CEL"))
        .send()
        .await
        .expect("create entity request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    for metadata in history_metadata(scope.database_pool(), &id, &default_context_iri("weight")).await {
        let holds_member = metadata.as_ref().and_then(|document| document.get(EXPANDED_UNIT_CODE)).is_some();
        assert!(
            !holds_member,
            "the unit member belongs to the column, not to the metadata document: {metadata:?}"
        );
    }

    // The whole hypertable, not only this test's rows: an identity with no
    // catalogue row is what a hand-written row or a catalogue regression leaves.
    let unregistered: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM attribute_history ah \
         LEFT JOIN unit_codes uc ON uc.id = ah.unit_code_id \
         WHERE ah.unit_code_id IS NOT NULL AND uc.id IS NULL",
    )
    .fetch_one(scope.database_pool())
    .await
    .expect("the unit audit must be readable");
    assert_eq!(unregistered, 0, "history holds unit identities the catalogue cannot decode");

    scope.finish().await.expect("cleanup failed");
}
