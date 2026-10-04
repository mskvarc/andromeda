//! An entity deletion notified under `showChanges` reports a deleted
//! `LanguageProperty` with a language map holding the NGSI-LD Null.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.5.18.2: an NGSI-LD Null "shall be encoded as
//! the JSON object {"@none": "urn:ngsi-ld:null"}. The same representation is
//! also used to indicate a deletion in notifications". Clause 5.8.6: when a
//! deleted Attribute is notified as an object because `showChanges` asks for
//! its previous value, "the languageMap is set to {"@none":
//! "urn:ngsi-ld:null"}". An entity deletion wrote the bare Null into the
//! `languageMap` of a single-instance `LanguageProperty`, in the normalized
//! and the concise representation alike; the simplified one cannot carry
//! `showChanges` (Table 5.2.14.1-1).
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::notification_adapter::NotificationAdapter;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

/// The core context every payload here is written against.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// How long the owed notification is waited for.
const ARRIVAL: Duration = Duration::from_secs(15);

/// Subscribe to the deletion of entities of `entity_type`, notified in
/// `format` under `showChanges`, and return the subscription identifier.
async fn subscribe(scope: &TestEnvironment, adapter: &NotificationAdapter, entity_type: &str, format: &str) -> String {
    let id = scope.id(&format!("Subscription:{entity_type}:{format}"));
    let body = json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": entity_type}],
        "notificationTrigger": ["entityDeleted"],
        "notification": {
            "format": format,
            "showChanges": true,
            "endpoint": {"uri": adapter.endpoint_url(), "accept": "application/json"}
        },
        "@context": CORE_CONTEXT
    });
    let created = scope.client().create_subscription(&body).await.expect("create subscription request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    id
}

/// The `street` member of the one entity `subscription` is notified with.
async fn notified_street(adapter: &NotificationAdapter, subscription: &str) -> Value {
    let notification = adapter.wait_for_subscription_notification(subscription, ARRIVAL).await;
    notification.body["data"][0]["street"].clone()
}

/// A single-instance `LanguageProperty` deleted with its entity is notified with
/// `{"@none": "urn:ngsi-ld:null"}` as its `languageMap` in the normalized and
/// the concise representation, beside its previous language map.
#[tokio::test]
async fn regression_r22_an_entity_deletion_notifies_a_deleted_language_property_with_a_null_language_map() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("DeletedStreet");
    let id = scope.id(&format!("{entity_type}:1"));
    let created = scope
        .client()
        .post_entity(&json!({
            "id": id,
            "type": entity_type,
            "street": {"type": "LanguageProperty", "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}},
            "@context": CORE_CONTEXT
        }))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let normalized = subscribe(&scope, &adapter, &entity_type, "normalized").await;
    let concise = subscribe(&scope, &adapter, &entity_type, "concise").await;

    let deleted = scope.client().delete_entity(&id).await.expect("delete entity request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{:?}", deleted.body);

    let null_map = json!({"@none": "urn:ngsi-ld:null"});
    let previous = json!({"fr": "Grand Place", "nl": "Grote Markt"});
    let normalized_street = notified_street(&adapter, &normalized).await;
    let concise_street = notified_street(&adapter, &concise).await;
    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    assert_eq!(normalized_street["type"], json!("LanguageProperty"), "{normalized_street}");
    assert_eq!(normalized_street["languageMap"], null_map, "{normalized_street}");
    assert_eq!(normalized_street["previousLanguageMap"], previous, "{normalized_street}");
    assert_eq!(concise_street["languageMap"], null_map, "{concise_street}");
    assert_eq!(concise_street["previousLanguageMap"], previous, "{concise_street}");
}
