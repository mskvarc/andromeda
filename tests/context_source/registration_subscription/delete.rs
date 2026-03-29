use crate::harness::{client::RequestHeaders, fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

const PREFIX: &str = "context_source::registration_subscription::delete";

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 042_01: Delete by ID ────────────────────────────────────────────

    // 042_01_01: Delete a context source registration subscription (5.11.6)
    trials.push(trial(&format!("{PREFIX}::042_01_01::delete_by_id"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:042_01_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&id);
        let sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &id);

        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_csource_subscription(&id).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx.client.retrieve_csource_subscription(&id, RequestHeaders::new()).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // ── 042_02: Delete with invalid URI ─────────────────────────────────

    // 042_02_01: Delete with an invalid URI (5.11.6)
    trials.push(trial(&format!("{PREFIX}::042_02_01::invalid_uri"), |ctx| async move {
        let resp = ctx.client.delete_csource_subscription("invalidUri").await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        assert!(body["title"].is_string(), "response should contain title");
    }));

    // ── 042_03: Delete unknown subscription ─────────────────────────────

    // 042_03_01: Delete an unknown subscription (5.11.6)
    trials.push(trial(&format!("{PREFIX}::042_03_01::unknown_id"), |ctx| async move {
        let resp = ctx.client.delete_csource_subscription("urn:ngsi-ld:Subscription:unknownSubscription").await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound");
        assert!(body["title"].is_string(), "response should contain title");
    }));
}
