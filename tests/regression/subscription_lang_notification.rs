//! A local notification applies the subscription's `lang` to every
//! `LanguageProperty` it carries.
//!
//! ETSI GS CIM 009 v1.9.1 Table 5.2.12-1: `lang` is the "language filter to be
//! applied to the query" of the notification (clause 4.15), so a
//! `LanguageProperty` of a notified entity is reduced to the language the
//! subscription names: a Property whose value is the string of that language,
//! carrying the `lang` it was chosen in, in place of the whole language map.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

use crate::notification_adapter::NotificationAdapter;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;
use std::time::Duration;

/// The core context every payload here is written against.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// How long the owed notification is waited for.
const ARRIVAL: Duration = Duration::from_secs(15);

/// A subscription with `lang` notifies an entity created after it with the
/// `LanguageProperty` reduced to that language.
#[tokio::test]
async fn regression_subscription_lang_a_local_notification_reduces_the_language_property() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("LangTower");
    let subscription = scope.id("Subscription:lang-notification");
    let created = scope
        .client()
        .create_subscription(&json!({
            "id": subscription,
            "type": "Subscription",
            "entities": [{"type": entity_type}],
            "lang": "de",
            "notification": {"format": "normalized", "endpoint": {"uri": adapter.endpoint_url(), "accept": "application/json"}},
            "@context": CORE_CONTEXT
        }))
        .await
        .expect("create subscription request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let id = scope.id(&format!("{entity_type}:1"));
    let posted = scope
        .client()
        .post_entity(&json!({
            "id": id,
            "type": entity_type,
            "name": {"type": "LanguageProperty", "languageMap": {"en": "Tower", "de": "Turm"}},
            "@context": CORE_CONTEXT
        }))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(posted.status, StatusCode::CREATED, "{:?}", posted.body);

    let notification = adapter.wait_for_subscription_notification(&subscription, ARRIVAL).await;
    let name = notification.body["data"][0]["name"].clone();
    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    assert_eq!(name["type"], json!("Property"), "{name}");
    assert_eq!(name["value"], json!("Turm"), "{name}");
    assert_eq!(name["lang"], json!("de"), "{name}");
}
