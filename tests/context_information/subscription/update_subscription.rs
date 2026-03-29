use crate::harness::{client::RequestHeaders, fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "subscription::update";

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
    // ── 029_01: Update with non-present or invalid ID ────────────────

    // 029_01_01: Update subscription with empty/not-present ID → 405 (5.8.2)
    trials.push(trial(&format!("{PREFIX}::029_01_01::not_present_id"), |ctx| async move {
        let fragment = fixtures::load_fragment("subscriptions/fragments/subscription-update.json");

        let resp = ctx
            .client
            .request(
                Method::PATCH,
                "/subscriptions/",
                Some(&fragment),
                RequestHeaders::new().content_type("application/json"),
            )
            .await;
        assert_eq!(resp.status, StatusCode::METHOD_NOT_ALLOWED, "update with empty ID should return 405");
    }));

    // 029_01_02: Update subscription with invalid ID → 400 (5.8.2)
    trials.push(trial(&format!("{PREFIX}::029_01_02::invalid_id"), |ctx| async move {
        let fragment = fixtures::load_fragment("subscriptions/fragments/subscription-update.json");

        let resp = ctx.client.update_subscription("InvalidUri", &fragment, RequestHeaders::new()).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid ID should return 400");

        let body: serde_json::Value = resp.json();
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
            "error type should be BadRequestData"
        );
    }));

    // ── 029_02: Update unknown subscription ──────────────────────────

    // 029_02_01: Update non-existent subscription → 404 (5.8.2)
    trials.push(trial(&format!("{PREFIX}::029_02_01::unknown_subscription"), |ctx| async move {
        let fragment = fixtures::load_fragment("subscriptions/fragments/subscription-update.json");

        let resp = ctx
            .client
            .update_subscription("urn:ngsi-ld:Subscription:unknowSubscription", &fragment, RequestHeaders::new())
            .await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND, "unknown subscription should return 404");

        let body: serde_json::Value = resp.json();
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound",
            "error type should be ResourceNotFound"
        );
    }));

    // ── 029_03: Update with invalid fragment ─────────────────────────

    // 029_03_01: Update subscription with invalid fragment → 400 (5.8.2)
    trials.push(trial(&format!("{PREFIX}::029_03_01::invalid_fragment"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:029_03_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&id);
        let sub = fixtures::load_fixture("subscriptions/subscription.jsonld", &id);
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("subscriptions/fragments/subscription-invalid-update.json");
        let resp = ctx.client.update_subscription(&id, &fragment, RequestHeaders::new()).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid fragment should return 400");

        let body: serde_json::Value = resp.json();
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
            "error type should be BadRequestData"
        );
    }));

    // ── 029_04: Update with null mandatory properties ────────────────

    // 029_04_01: Update subscription setting mandatory fields to null → 400 (5.8.2)
    trials.push(trial(&format!("{PREFIX}::029_04_01::null_mandatory_properties"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:029_04_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&id);
        let sub = fixtures::load_fixture("subscriptions/subscription.jsonld", &id);
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("subscriptions/fragments/subscription-null-properties-update.json");
        let resp = ctx.client.update_subscription(&id, &fragment, RequestHeaders::new()).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "null mandatory properties should return 400");

        let body: serde_json::Value = resp.json();
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
            "error type should be BadRequestData"
        );
    }));

    // ── 029_05: Update with term to URI expansion ────────────────────

    // 029_05_01: Update subscription, verify with context (5.8.2)
    trials.push(trial(&format!("{PREFIX}::029_05_01::term_to_uri_expansion_with_context"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:029_05_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&id);
        let sub = fixtures::load_fixture("subscriptions/subscription.jsonld", &id);
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("subscriptions/fragments/subscription-vehicle-entities.json");
        let resp = ctx
            .client
            .update_subscription(&id, &fragment, RequestHeaders::new().link_context(fixtures::test_suite_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT, "update should return 204");

        let resp = ctx
            .client
            .retrieve_subscription(&id, RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["jsonldContext", "timesFailed", "timesSent", "notificationTrigger", "@context"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("subscriptions/expectations/subscription-vehicle.jsonld");
        expected["id"] = serde_json::Value::String(id.clone());
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // 029_05_02: Update subscription, verify without context (5.8.2)
    trials.push(trial(
        &format!("{PREFIX}::029_05_02::term_to_uri_expansion_without_context"),
        |ctx| async move {
            let id = format!("urn:ngsi-ld:Subscription:029_05_02:{}", uuid::Uuid::new_v4());
            ctx.track_subscription(&id);
            let sub = fixtures::load_fixture("subscriptions/subscription.jsonld", &id);
            let resp = ctx.client.create_subscription(&sub).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let fragment = fixtures::load_fragment("subscriptions/fragments/subscription-vehicle-entities.json");
            let resp = ctx
                .client
                .update_subscription(&id, &fragment, RequestHeaders::new().link_context(fixtures::test_suite_context_url()))
                .await;
            assert_eq!(resp.status, StatusCode::NO_CONTENT, "update should return 204");

            let resp = ctx.client.retrieve_subscription(&id, RequestHeaders::new().accept_ld_json()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let ignored = &["jsonldContext", "timesFailed", "timesSent", "notificationTrigger", "@context"];
            let mut actual: serde_json::Value = resp.json();
            let mut expected = fixtures::load_expectation("subscriptions/expectations/subscription-vehicle.jsonld");
            expected["id"] = serde_json::Value::String(id.clone());
            strip_ignored_keys(&mut actual, ignored);
            strip_ignored_keys(&mut expected, ignored);
            assert_eq!(expected, actual);
        },
    ));

    // ── 029_06: Update subscription ──────────────────────────────────

    // 029_06_01: Update subscription and verify modified content (5.8.2)
    trials.push(trial(&format!("{PREFIX}::029_06_01::update_subscription"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:029_06_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&id);
        let sub = fixtures::load_fixture("subscriptions/subscription.jsonld", &id);
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("subscriptions/fragments/subscription-update.json");
        let resp = ctx
            .client
            .update_subscription(&id, &fragment, RequestHeaders::new().link_context(fixtures::test_suite_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT, "update should return 204");

        let resp = ctx
            .client
            .retrieve_subscription(&id, RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        // Build expected: original subscription with updated notification endpoint
        let mut expected = sub.clone();
        expected["notification"]["format"] = serde_json::json!("keyValues");
        expected["notification"]["endpoint"]["uri"] = serde_json::json!("http://my.second.endpoint.org/notify");

        let ignored = &["status", "jsonldContext", "timesFailed", "timesSent", "notificationTrigger", "@context"];
        let mut actual: serde_json::Value = resp.json();
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // ── 029_07: Activate paused subscription with isActive=true ──────

    // 029_07_01: Activate paused subscription (5.8.2)
    trials.push(trial(&format!("{PREFIX}::029_07_01::activate_paused_with_isActive_true"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:029_07_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&id);
        let sub = fixtures::load_fixture("subscriptions/subscription-inactive.jsonld", &id);
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("subscriptions/fragments/subscription-isActive-true-update.json");
        let resp = ctx.client.update_subscription(&id, &fragment, RequestHeaders::new()).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT, "update should return 204");

        let resp = ctx.client.retrieve_subscription(&id, RequestHeaders::new().accept_ld_json()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["status"], "active", "status should be active");
    }));

    // ── 029_08: Activate paused subscription with isActive and expiresAt

    // 029_08_01: Activate paused subscription with future expiresAt (5.8.2)
    trials.push(trial(
        &format!("{PREFIX}::029_08_01::activate_paused_with_isActive_and_expiresAt"),
        |ctx| async move {
            let id = format!("urn:ngsi-ld:Subscription:029_08_01:{}", uuid::Uuid::new_v4());
            ctx.track_subscription(&id);
            let sub = fixtures::load_fixture("subscriptions/subscription-inactive.jsonld", &id);
            let resp = ctx.client.create_subscription(&sub).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let fragment = fixtures::load_fragment("subscriptions/fragments/subscription-isActive-expiresAt-update.json");
            let resp = ctx.client.update_subscription(&id, &fragment, RequestHeaders::new()).await;
            assert_eq!(resp.status, StatusCode::NO_CONTENT, "update should return 204");

            let resp = ctx.client.retrieve_subscription(&id, RequestHeaders::new().accept_ld_json()).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body: serde_json::Value = resp.json();
            assert_eq!(body["status"], "active", "status should be active");
        },
    ));

    // ── 029_09: Pause active subscription ────────────────────────────

    // 029_09_01: Update active subscription to paused (5.8.2)
    trials.push(trial(&format!("{PREFIX}::029_09_01::pause_active_subscription"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:029_09_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&id);
        let sub = fixtures::load_fixture("subscriptions/subscription.jsonld", &id);
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("subscriptions/fragments/subscription-isActive-false-update.json");
        let resp = ctx.client.update_subscription(&id, &fragment, RequestHeaders::new()).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT, "update should return 204");

        let resp = ctx.client.retrieve_subscription(&id, RequestHeaders::new().accept_ld_json()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["status"], "paused", "status should be paused");
    }));

    // ── 029_10: Activate expired subscription ────────────────────────

    // 029_10_01: Expire then re-activate subscription (5.8.2)
    trials.push(trial(&format!("{PREFIX}::029_10_01::activate_expired_subscription"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:029_10_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&id);
        let sub = fixtures::load_fixture("subscriptions/subscription.jsonld", &id);
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Set expiresAt to 5 seconds in the future
        let expires_at = chrono::Utc::now() + chrono::Duration::seconds(5);
        let expire_fragment = serde_json::json!({
            "type": "Subscription",
            "expiresAt": expires_at.format("%Y-%m-%dT%H:%M:%SZ").to_string()
        });
        let resp = ctx.client.update_subscription(&id, &expire_fragment, RequestHeaders::new()).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT, "first update should return 204");

        // Wait for the subscription to expire
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;

        // Re-activate with a far-future expiresAt
        let fragment = fixtures::load_fragment("subscriptions/fragments/subscription-expiresAt-future-update.json");
        let resp = ctx.client.update_subscription(&id, &fragment, RequestHeaders::new()).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT, "second update should return 204");

        let resp = ctx.client.retrieve_subscription(&id, RequestHeaders::new().accept_ld_json()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["status"], "active", "status should be active after re-activation");
    }));

    // ── 029_11: Update with expiresAt in the past ────────────────────

    // 029_11_01: Update subscription with past expiresAt → 400 (5.8.2)
    trials.push(trial(&format!("{PREFIX}::029_11_01::expires_at_in_the_past"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:029_11_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&id);
        let sub = fixtures::load_fixture("subscriptions/subscription.jsonld", &id);
        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("subscriptions/fragments/subscription-expiresAt-update.json");
        let resp = ctx.client.update_subscription(&id, &fragment, RequestHeaders::new()).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "expiresAt in the past should return 400");

        let body: serde_json::Value = resp.json();
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
            "error type should be BadRequestData"
        );
    }));
}
