use crate::harness::{client::RequestHeaders, fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

const PREFIX: &str = "subscription::retrieve";

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
    // ── 030_01: Retrieve with invalid ID ─────────────────────────────

    // 030_01_01: Retrieve subscription with invalid ID → 400 (5.8.3)
    trials.push(trial(&format!("{PREFIX}::030_01_01::invalid_id"), |ctx| async move {
        let resp = ctx.client.retrieve_subscription("InvalidUri", RequestHeaders::new()).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid ID should return 400");

        let body: serde_json::Value = resp.json();
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
            "error type should be BadRequestData"
        );
    }));

    // ── 030_02: Retrieve unknown subscription ────────────────────────

    // 030_02_01: Retrieve non-existent subscription → 404 (5.8.3)
    trials.push(trial(&format!("{PREFIX}::030_02_01::unknown_subscription"), |ctx| async move {
        let resp = ctx
            .client
            .retrieve_subscription("urn:ngsi-ld:Subscription:unknowSubscription", RequestHeaders::new())
            .await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND, "unknown subscription should return 404");

        let body: serde_json::Value = resp.json();
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound",
            "error type should be ResourceNotFound"
        );
    }));

    // ── 030_03: Retrieve subscription ────────────────────────────────

    // 030_03_01: Retrieve existing subscription and verify content (5.8.3)
    trials.push(trial(&format!("{PREFIX}::030_03_01::retrieve_subscription"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:030_03_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&id);
        let sub = fixtures::load_fixture("subscriptions/subscription.jsonld", &id);

        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .retrieve_subscription(&id, RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["jsonldContext", "timesFailed", "timesSent", "notificationTrigger", "@context"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("subscriptions/expectations/subscriptions-030-03.json");
        expected["id"] = serde_json::Value::String(id.clone());
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));
}
