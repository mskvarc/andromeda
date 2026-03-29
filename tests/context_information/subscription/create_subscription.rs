use crate::harness::{client::RequestHeaders, fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "subscription::create";

/// Recursively strip keys that vary between implementations for comparison.
fn strip_ignored_keys(val: &mut serde_json::Value, keys: &[&str]) {
    match val {
        serde_json::Value::Object(map) => {
            for key in keys {
                map.remove(*key);
            }
            for v in map.values_mut() {
                strip_ignored_keys(v, keys);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                strip_ignored_keys(v, keys);
            }
        }
        _ => {}
    }
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 028_01: Create subscription ────────────────────────────────────

    // 028_01_01: Create a subscription and verify its content
    trials.push(trial(&format!("{PREFIX}::028_01_01::create_subscription"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:028_01_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&id);
        let sub = fixtures::load_fixture("subscriptions/subscription.jsonld", &id);

        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.retrieve_subscription(&id, RequestHeaders::new().accept_ld_json()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["jsonldContext", "timesFailed", "timesSent", "notificationTrigger", "@context", "status"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("subscriptions/expectations/subscription-028-01.jsonld");
        // Set the expected ID to match our generated one
        expected["id"] = serde_json::Value::String(id.clone());
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // ── 028_02: Invalid request ────────────────────────────────────────

    // 028_02_01: Create subscription with invalid JSON → 400
    trials.push(trial(&format!("{PREFIX}::028_02_01::invalid_json"), |ctx| async move {
        let raw = fixtures::load_fixture_raw("subscriptions/subscription-invalid-json.jsonld");

        let resp = ctx
            .client
            .request_raw(Method::POST, "/subscriptions", raw.as_bytes(), "application/ld+json")
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid JSON should return 400");

        let body: serde_json::Value = resp.json();
        assert!(
            body["type"].as_str().unwrap_or("").contains("InvalidRequest"),
            "error type should be InvalidRequest, got: {}",
            body["type"]
        );
    }));

    // 028_02_02: Create subscription with empty JSON → 400
    trials.push(trial(&format!("{PREFIX}::028_02_02::empty_json"), |ctx| async move {
        let empty = serde_json::json!({});

        let resp = ctx.client.create_subscription(&empty).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "empty JSON should return 400");

        let body: serde_json::Value = resp.json();
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
            "error type should be BadRequestData"
        );
    }));

    // ── 028_03: Invalid/empty ID ───────────────────────────────────────

    // 028_03_01: Create subscription with invalid ID → 400
    trials.push(trial(&format!("{PREFIX}::028_03_01::invalid_id"), |ctx| async move {
        let sub = serde_json::json!({
            "id": "invalidId",
            "type": "Subscription",
            "entities": [{"type": "Building"}],
            "notification": {
                "endpoint": {
                    "uri": "http://127.0.0.1:8085/notify",
                    "accept": "application/json"
                }
            },
            "@context": [fixtures::test_suite_context_url()]
        });

        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid ID should return 400");

        let body: serde_json::Value = resp.json();
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
            "error type should be BadRequestData"
        );
    }));

    // 028_03_02: Create subscription with empty ID → 400
    trials.push(trial(&format!("{PREFIX}::028_03_02::empty_id"), |ctx| async move {
        let sub = serde_json::json!({
            "id": "",
            "type": "Subscription",
            "entities": [{"type": "Building"}],
            "notification": {
                "endpoint": {
                    "uri": "http://127.0.0.1:8085/notify",
                    "accept": "application/json"
                }
            },
            "@context": [fixtures::test_suite_context_url()]
        });

        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "empty ID should return 400");

        let body: serde_json::Value = resp.json();
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
            "error type should be BadRequestData"
        );
    }));

    // ── 028_04: Duplicate subscription ─────────────────────────────────

    // 028_04_01: Create subscription with existing ID → 409
    trials.push(trial(&format!("{PREFIX}::028_04_01::duplicate_id"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:028_04_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&id);
        let sub = fixtures::load_fixture("subscriptions/subscription.jsonld", &id);

        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CONFLICT, "duplicate should return 409");

        let body: serde_json::Value = resp.json();
        assert!(
            body["type"].as_str().unwrap_or("").contains("AlreadyExists"),
            "error type should be AlreadyExists, got: {}",
            body["type"]
        );
    }));

    // ── 028_05: Invalid throttling ─────────────────────────────────────

    // 028_05_01: Throttling and timeInterval conjunction → 400
    trials.push(trial(&format!("{PREFIX}::028_05_01::throttling_and_time_interval"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:028_05_01:{}", uuid::Uuid::new_v4());
        let sub = fixtures::load_fixture("subscriptions/subscription-invalid-throttling-timeInterval.jsonld", &id);

        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "throttling+timeInterval should return 400");

        let body: serde_json::Value = resp.json();
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
            "error type should be BadRequestData"
        );
    }));

    // 028_05_02: Negative throttling → 400
    trials.push(trial(&format!("{PREFIX}::028_05_02::negative_throttling"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:028_05_02:{}", uuid::Uuid::new_v4());
        let sub = fixtures::load_fixture("subscriptions/subscription-invalid-negative-throttling.jsonld", &id);

        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "negative throttling should return 400");

        let body: serde_json::Value = resp.json();
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
            "error type should be BadRequestData"
        );
    }));

    // ── 028_06: Subscription with datasetId ────────────────────────────

    // 028_06_01: Create subscription with datasetId and verify content
    trials.push(trial(&format!("{PREFIX}::028_06_01::with_dataset_id"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:028_06_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&id);
        let sub = fixtures::load_fixture("subscriptions/subscription-building-with-datasetId.jsonld", &id);

        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.retrieve_subscription(&id, RequestHeaders::new().accept_ld_json()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "timesFailed", "timesSent"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("subscriptions/expectations/subscription-with-datasetId.jsonld");
        expected["id"] = serde_json::Value::String(id.clone());
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // ── 028_07: Invalid/unavailable jsonldContext ──────────────────────

    // 028_07_01: Subscription with invalid jsonldContext → 400
    trials.push(trial(&format!("{PREFIX}::028_07_01::invalid_jsonld_context"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:028_07_01:{}", uuid::Uuid::new_v4());
        let sub = fixtures::load_fixture("subscriptions/subscription-building-with-invalid-jsonldContext.jsonld", &id);

        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid jsonldContext should return 400");

        let body: serde_json::Value = resp.json();
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
            "error type should be BadRequestData"
        );
    }));

    // 028_07_02: Subscription with unavailable jsonldContext → 503
    trials.push(trial(&format!("{PREFIX}::028_07_02::unavailable_jsonld_context"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:028_07_02:{}", uuid::Uuid::new_v4());
        let sub = fixtures::load_fixture("subscriptions/subscription-building-with-unavailable-jsonldContext.jsonld", &id);

        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::SERVICE_UNAVAILABLE, "unavailable jsonldContext should return 503");

        let body: serde_json::Value = resp.json();
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/LdContextNotAvailable",
            "error type should be LdContextNotAvailable"
        );
    }));
}
