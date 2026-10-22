//! Invalid unit: a `unitCode` that is no UN/CEFACT common code is refused.
//!
//! A string that names no common code has no identity to store, so the write
//! is refused rather than silently stripped: an attribute persisted without
//! the unit its client declared would mean something else. The database
//! refuses a non-positive identity from a writer that bypasses the broker,
//! and no history row may carry a unit identity no listed code holds.

use crate::fixtures::{SUITE, default_context_iri, device_with_unit, history_unit_identities};
use andromeda_postgresql_schema::{
    current::{CurrentAttributes, Entities},
    temporal::AttributeHistory,
};
use andromeda_postgresql_seaquery::{scalar_subquery::scalar_subquery, statement::Statement};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use cefact_units::UnitCode;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use sea_query::{Expr, ExprTrait, Query};
use serde_json::json;

/// A three-letter token of the right shape that Recommendation 20 never assigned.
const NOT_A_COMMON_CODE: &str = "QQQ";

#[tokio::test]
async fn a_property_carrying_a_string_that_is_no_common_code_is_refused_and_persists_nothing() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:invalid-unit");

    let entity = json!({
        "id": id,
        "type": "Device",
        "weight": {"type": "Property", "value": 5, "unitCode": NOT_A_COMMON_CODE},
        "@context": [CORE_CONTEXT]
    });
    let created = client.post_entity(&entity).send().await.expect("create entity request failed");
    assert_ne!(
        created.status,
        StatusCode::CREATED,
        "a string that is no common code must not be accepted: {:?}",
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
async fn a_string_that_is_no_common_code_is_refused_as_bad_request_data() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:invalid-unit-status");

    let entity = json!({
        "id": id,
        "type": "Device",
        "weight": {"type": "Property", "value": 5, "unitCode": NOT_A_COMMON_CODE},
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
async fn a_bypass_write_of_a_non_positive_unit_identity_is_refused_by_the_check() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:bypass-unit");

    let created = client
        .post_entity(&device_with_unit(&id, "KGM", "KGM"))
        .send()
        .await
        .expect("create entity request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let bypass = Query::update()
        .table(CurrentAttributes::Table)
        .value(CurrentAttributes::UnitCodeId, 0)
        .and_where(
            Expr::col(CurrentAttributes::EntityKey).eq(scalar_subquery(
                Query::select()
                    .column(Entities::EntityKey)
                    .from(Entities::Table)
                    .and_where(Expr::col(Entities::Id).eq(id.as_str()))
                    .to_owned(),
            )),
        )
        .to_query()
        .execute(scope.database_pool())
        .await;

    let refusal = bypass.expect_err("the check must refuse a non-positive unit identity");
    let database_error = refusal.as_database_error().expect("the refusal comes from the database");
    assert_eq!(
        database_error.code().as_deref(),
        Some("23514"),
        "the refusal must be a check violation: {refusal}"
    );
    assert_eq!(database_error.constraint(), Some("chk_current_attributes_unit_code_id_positive"));

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

    // The whole hypertable, not only this test's rows: an identity no listed
    // code holds is what a hand-written row leaves.
    let stored: Vec<i16> = Query::select()
        .distinct()
        .column(AttributeHistory::UnitCodeId)
        .from(AttributeHistory::Table)
        .and_where(Expr::col(AttributeHistory::UnitCodeId).is_not_null())
        .to_query_scalar()
        .fetch_all(scope.database_pool())
        .await
        .expect("the unit audit must be readable");
    for identity in stored {
        assert!(
            UnitCode::from_smallint(identity).is_some(),
            "history holds unit identity {identity}, which no listed code holds"
        );
    }

    scope.finish().await.expect("cleanup failed");
}
