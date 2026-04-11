use crate::harness::{client::RequestHeaders, fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "context_source::registration_subscription::update";

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 039_01: Update subscription ─────────────────────────────────────

    // 039_01_01: Update a context source registration subscription (5.11.3)
    trials.push(trial(&format!("{PREFIX}::039_01_01::update_subscription"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:039_01_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&id);
        let sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &id);

        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("csource_subscriptions/fragments/subscription-update.json");
        let resp = ctx
            .client
            .update_csource_subscription(&id, &fragment, RequestHeaders::new().link_context(fixtures::compound_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);
    }));

    // ── 039_02: Update with invalid URI ─────────────────────────────────

    // 039_02_01: Update with an invalid URI (5.11.3)
    trials.push(trial(&format!("{PREFIX}::039_02_01::invalid_uri"), |ctx| async move {
        let fragment = fixtures::load_fragment("csource_subscriptions/fragments/subscription-update.json");
        let resp = ctx.client.update_csource_subscription("invalidUri", &fragment, RequestHeaders::new()).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        assert!(body["title"].is_string(), "response should contain title");
    }));

    // ── 039_03: Update unknown subscription ─────────────────────────────

    // 039_03_01: Update an unknown subscription (5.11.3)
    trials.push(trial(&format!("{PREFIX}::039_03_01::unknown_id"), |ctx| async move {
        let fragment = fixtures::load_fragment("csource_subscriptions/fragments/subscription-update.json");
        let resp = ctx
            .client
            .update_csource_subscription("urn:ngsi-ld:Subscription:unknownSubscription", &fragment, RequestHeaders::new())
            .await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound");
        assert!(body["title"].is_string(), "response should contain title");
    }));

    // ── 039_04: Update with invalid fragment ────────────────────────────

    // 039_04_01: Invalid type in fragment (5.11.3)
    trials.push(trial(&format!("{PREFIX}::039_04_01::invalid_type_fragment"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:039_04_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&id);
        let sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &id);

        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("csource_subscriptions/fragments/subscription-update-invalid-type.json");
        let resp = ctx
            .client
            .update_csource_subscription(&id, &fragment, RequestHeaders::new().link_context(fixtures::compound_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        assert!(body["title"].is_string(), "response should contain title");
    }));

    // 039_04_02: Invalid notification in fragment (5.11.3)
    trials.push(trial(&format!("{PREFIX}::039_04_02::invalid_notification_fragment"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:039_04_02:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&id);
        let sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &id);

        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("csource_subscriptions/fragments/subscription-update-invalid-notification.json");
        let resp = ctx
            .client
            .update_csource_subscription(&id, &fragment, RequestHeaders::new().link_context(fixtures::compound_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        assert!(body["title"].is_string(), "response should contain title");
    }));

    // ── 039_05: Update with invalid JSON ────────────────────────────────

    // 039_05_01: Malformed JSON body (5.11.3)
    trials.push(trial(&format!("{PREFIX}::039_05_01::invalid_json"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:039_05_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&id);
        let sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &id);

        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let raw = fixtures::load_fixture_raw("csource_subscriptions/fragments/subscription-update-invalid-json.json");
        let resp = ctx
            .client
            .request_raw(Method::PATCH, &format!("/csourceSubscriptions/{id}"), raw.as_bytes(), "application/json")
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        // InvalidRequest: the body is syntactically malformed JSON, consistent with
        // all other endpoints that use the JsonLd extractor for body parsing.
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest");
        assert!(body["title"].is_string(), "response should contain title");
    }));
}
