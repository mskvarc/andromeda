use crate::harness::{client::RequestHeaders, fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "subscription::delete";

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 032_01: Delete with non-present or invalid ID ──────────────────

    // 032_01_01: Delete subscription with empty/not-present ID → 405
    trials.push(trial(&format!("{PREFIX}::032_01_01::not_present_id"), |ctx| async move {
        let resp = ctx.client.request(Method::DELETE, "/subscriptions/", None, RequestHeaders::new()).await;
        assert_eq!(resp.status, StatusCode::METHOD_NOT_ALLOWED, "delete with empty ID should return 405");
    }));

    // 032_01_02: Delete subscription with invalid ID → 400
    trials.push(trial(&format!("{PREFIX}::032_01_02::invalid_id"), |ctx| async move {
        let resp = ctx.client.delete_subscription("InvalidUri").await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid ID should return 400");

        let body: serde_json::Value = resp.json();
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
            "error type should be BadRequestData"
        );
    }));

    // ── 032_02: Delete unknown subscription ────────────────────────────

    // 032_02_01: Delete non-existent subscription → 404
    trials.push(trial(&format!("{PREFIX}::032_02_01::unknown_subscription"), |ctx| async move {
        let resp = ctx.client.delete_subscription("urn:ngsi-ld:Subscription:unknowSubscription").await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND, "unknown subscription should return 404");

        let body: serde_json::Value = resp.json();
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound",
            "error type should be ResourceNotFound"
        );
    }));

    // ── 032_03: Delete subscription ────────────────────────────────────

    // 032_03_01: Delete existing subscription → 204, then verify 404 on retrieval
    trials.push(trial(&format!("{PREFIX}::032_03_01::delete_subscription"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:032_03_01:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&id);
        let sub = fixtures::load_fixture("subscriptions/subscription.jsonld", &id);

        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_subscription(&id).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT, "delete should return 204");
        assert!(resp.body_bytes().is_empty(), "response body should be empty");

        // Verify the subscription is gone
        let resp = ctx.client.retrieve_subscription(&id, RequestHeaders::new().accept_ld_json()).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND, "deleted subscription should return 404");
    }));
}
