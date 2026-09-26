//! Verification matrix row "Code registry stability".
//!
//! The compact code domains are numbers a migration writes into `SMALLINT`
//! columns on rows nobody will rewrite, so their meaning is fixed by the
//! migration rather than by whatever the current Rust source says. The
//! append-only evolution of each registry is enforced by the generated
//! stability tests inside `andromeda-postgresql-codes`; what only a migrated
//! tenant can answer is whether the numbers actually stored agree with those
//! registries, and whether the schema still refuses a number no release has
//! assigned.

use crate::SUITE;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, csr_operation::CsrOperationCode, scope_value_kind::ScopeValueKind, value_kind::ValueKind};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

#[tokio::test]
async fn every_stored_code_decodes_through_the_registry_that_owns_its_column() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();

    let attribute_kinds: Vec<i16> = sqlx::query_scalar("SELECT DISTINCT attribute_kind FROM attribute_history")
        .fetch_all(pool)
        .await
        .expect("history attribute kinds must be readable");
    for stored in attribute_kinds {
        assert!(
            AttributeKind::try_from(stored).is_ok(),
            "history holds attribute kind {stored}, which no release assigned"
        );
    }

    let value_kinds: Vec<i16> = sqlx::query_scalar("SELECT DISTINCT value_kind FROM attribute_history")
        .fetch_all(pool)
        .await
        .expect("history value kinds must be readable");
    for stored in value_kinds {
        assert!(
            ValueKind::try_from(stored).is_ok(),
            "history holds value kind {stored}, which no release assigned"
        );
    }

    let projected_kinds: Vec<i16> = sqlx::query_scalar("SELECT DISTINCT value_kind FROM current_attribute_values")
        .fetch_all(pool)
        .await
        .expect("projected value kinds must be readable");
    for stored in projected_kinds {
        assert!(
            ValueKind::try_from(stored).is_ok(),
            "the current projection holds value kind {stored}, which no release assigned"
        );
    }

    let scope_kinds: Vec<i16> = sqlx::query_scalar("SELECT DISTINCT value_kind FROM scope_history")
        .fetch_all(pool)
        .await
        .expect("scope value kinds must be readable");
    for stored in scope_kinds {
        assert!(
            ScopeValueKind::try_from(stored).is_ok(),
            "scope history holds value kind {stored}, which no release assigned"
        );
    }

    let operations: Vec<i16> = sqlx::query_scalar("SELECT DISTINCT unnest(operations) FROM context_source_registrations WHERE operations IS NOT NULL")
        .fetch_all(pool)
        .await
        .expect("registered operations must be readable");
    for stored in operations {
        assert!(
            CsrOperationCode::try_from(stored).is_ok(),
            "a registration holds operation {stored}, which no release assigned"
        );
    }

    scope.finish().await.expect("cleanup failed");
}

/// The tombstone is permanently reserved at zero in both value registries, and
/// a tombstone row is exactly a row carrying that number and no typed value
/// (clauses 4.5.7 and 4.5.8).
#[tokio::test]
async fn the_tombstone_code_stays_zero_and_names_rows_carrying_no_typed_value() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");

    assert_eq!(ValueKind::Tombstone.code(), 0, "the attribute tombstone is reserved at zero");
    assert_eq!(ScopeValueKind::Tombstone.code(), 0, "the scope tombstone is reserved at zero");

    let typed_tombstones: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM attribute_history \
         WHERE value_kind = 0 AND (value_number IS NOT NULL OR value_string IS NOT NULL OR value_bool IS NOT NULL \
             OR value_datetime IS NOT NULL OR value_geo IS NOT NULL OR value_object IS NOT NULL OR value_uri IS NOT NULL)",
    )
    .fetch_one(scope.database_pool())
    .await
    .expect("the tombstone audit must be readable");
    assert_eq!(typed_tombstones, 0, "a tombstone row carries no typed value at all");

    scope.finish().await.expect("cleanup failed");
}

/// A registration pins the operation registry's range in a check constraint,
/// so a number outside it -- which could only come from a release knowing an
/// operation this schema does not -- is refused by the database.
#[tokio::test]
async fn a_registration_refuses_an_operation_number_no_release_assigned() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let registration_id = scope.id("ContextSourceRegistration:code-registry");

    let registration = json!({
        "id": registration_id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Device"}]}],
        "endpoint": "http://localhost:9/csource",
        "@context": [CORE_CONTEXT]
    });
    let created = client.create_registration(&registration).await.expect("create registration request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create registration: {:?}", created.body);

    let highest = CsrOperationCode::ALL.last().expect("the operation registry is not empty").code();
    let unassigned = highest + 1;
    let refused = sqlx::query("UPDATE context_source_registrations SET operations = ARRAY[$2::SMALLINT] WHERE id = $1")
        .bind(&registration_id)
        .bind(unassigned)
        .execute(scope.database_pool())
        .await;
    let violation = refused.expect_err("an unassigned operation number must be refused");
    let code = violation.as_database_error().and_then(|error| error.code().map(std::borrow::Cow::into_owned));
    assert_eq!(code.as_deref(), Some("23514"), "the refusal must be a check violation: {violation}");

    // The highest number the registry does assign is accepted, so the check
    // pins the registry's range rather than an arbitrary smaller one.
    let accepted = sqlx::query("UPDATE context_source_registrations SET operations = ARRAY[$2::SMALLINT] WHERE id = $1")
        .bind(&registration_id)
        .bind(highest)
        .execute(scope.database_pool())
        .await;
    assert!(accepted.is_ok(), "the registry's own highest operation must be storable: {accepted:?}");

    let deleted = client.delete_registration(&registration_id).await.expect("delete registration request failed");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete registration: {:?}", deleted.body);

    scope.finish().await.expect("cleanup failed");
}
