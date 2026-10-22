//! The entity-level `expiresAt` the write path records for the expiry sweep
//! (clause 4.22).
//!
//! The sweep deletes an entity whose recorded expiry has passed without
//! reading its document again, so the recorded instant has to follow the
//! document every write stores: a write whose document no longer carries
//! `expiresAt` leaves no instant behind.

use crate::{create, fetch, recorded_expiry};
use andromeda_postgresql_schema::current::Entities;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

/// A Replace Entity (clause 5.6.18) whose replacement carries no `expiresAt`
/// leaves the entity without one, so the recorded instant is cleared rather
/// than kept for the sweep to delete the entity at.
#[tokio::test]
async fn expiration_replacing_an_entity_without_expires_at_clears_its_recorded_expiry() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-replaced");
    create(
        client,
        &json!({
            "id": id,
            "type": "Vehicle",
            "expiresAt": "2099-01-01T00:00:00Z",
            "speed": {"type": "Property", "value": 60},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    assert_eq!(recorded_expiry(&scope, &id, Entities::ExpiresAt).await.as_deref(), Some("2099-01-01T00:00:00Z"));

    let replaced = client
        .replace_entity(
            &id,
            &json!({
                "id": id,
                "type": "Vehicle",
                "speed": {"type": "Property", "value": 70},
                "@context": [TEST_SUITE_CONTEXT]
            }),
            "application/ld+json",
            None,
        )
        .await
        .expect("replace_entity failed");
    assert_eq!(replaced.status, StatusCode::NO_CONTENT, "{:?}", replaced.body);

    assert_eq!(
        recorded_expiry(&scope, &id, Entities::ExpiresAt).await,
        None,
        "an expiry the stored document dropped must not stay recorded"
    );
    let body = fetch(client, &id, "options=sysAttrs").await;
    assert!(body.get("expiresAt").is_none(), "{body}");
    scope.finish().await.expect("cleanup failed");
}

/// A replacement that moves the entity's `expiresAt` records the new instant.
#[tokio::test]
async fn expiration_replacing_an_entity_with_a_new_expires_at_records_the_new_instant() {
    let scope = TestEnvironment::from_suite("expiration").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:exp-moved");
    create(
        client,
        &json!({
            "id": id,
            "type": "Vehicle",
            "expiresAt": "2099-01-01T00:00:00Z",
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let replaced = client
        .replace_entity(
            &id,
            &json!({
                "id": id,
                "type": "Vehicle",
                "expiresAt": "2098-06-01T00:00:00Z",
                "@context": [TEST_SUITE_CONTEXT]
            }),
            "application/ld+json",
            None,
        )
        .await
        .expect("replace_entity failed");
    assert_eq!(replaced.status, StatusCode::NO_CONTENT, "{:?}", replaced.body);

    assert_eq!(recorded_expiry(&scope, &id, Entities::ExpiresAt).await.as_deref(), Some("2098-06-01T00:00:00Z"));
    scope.finish().await.expect("cleanup failed");
}
