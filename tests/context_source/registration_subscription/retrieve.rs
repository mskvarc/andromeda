use crate::harness::{client::RequestHeaders, fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

const PREFIX: &str = "context_source::registration_subscription::retrieve";

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 040_01: Retrieve by ID ──────────────────────────────────────────

    // 040_01_01: Retrieve a context source registration subscription (5.11.4)
    trials.push(trial(&format!("{PREFIX}::040_01_01::retrieve_by_id"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:040_01_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&id);
        let sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &id);

        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .retrieve_csource_subscription(&id, RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["id"], id);
        assert_eq!(body["type"], "Subscription");
    }));

    // ── 040_02: Invalid ID ──────────────────────────────────────────────

    // 040_02_01: Retrieve with an invalid URI (5.11.4)
    trials.push(trial(&format!("{PREFIX}::040_02_01::invalid_id"), |ctx| async move {
        let resp = ctx.client.retrieve_csource_subscription("invalidUri", RequestHeaders::new()).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        assert!(body["title"].is_string(), "response should contain title");
    }));

    // ── 040_03: Unknown ID ──────────────────────────────────────────────

    // 040_03_01: Retrieve an unknown subscription (5.11.4)
    trials.push(trial(&format!("{PREFIX}::040_03_01::unknown_id"), |ctx| async move {
        let resp = ctx
            .client
            .retrieve_csource_subscription("urn:ngsi-ld:Subscription:unknownSubscription", RequestHeaders::new())
            .await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound");
        assert!(body["title"].is_string(), "response should contain title");
    }));
}
