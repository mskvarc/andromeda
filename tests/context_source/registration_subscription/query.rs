use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::{trial, trial_clean_db},
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "context_source::registration_subscription::query";

/// Helper to create N csource subscriptions, returning their IDs.
async fn create_subscriptions(ctx: &crate::harness::TestCtx, count: usize) -> Vec<String> {
    let mut ids = Vec::with_capacity(count);
    for i in 0..count {
        let id = format!("urn:ngsi-ld:Subscription:query_{i}:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&id);
        let sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &id);
        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ids.push(id);
    }
    ids
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 041_01: Query all ───────────────────────────────────────────────

    // 041_01_01: Query context source registration subscriptions (5.11.5)
    trials.push(trial_clean_db(&format!("{PREFIX}::041_01_01::query_all"), |ctx| async move {
        create_subscriptions(&ctx, 2).await;

        let resp = ctx
            .client
            .query_csource_subscriptions(
                &QueryParams::new(),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("response should be an array");
        assert!(arr.len() >= 2, "expected at least 2 subscriptions, got {}", arr.len());
    }));

    // ── 041_02: Query with limit ────────────────────────────────────────

    // 041_02_01: limit=1 (5.11.5)
    trials.push(trial_clean_db(&format!("{PREFIX}::041_02_01::limit_1"), |ctx| async move {
        create_subscriptions(&ctx, 3).await;

        let resp = ctx
            .client
            .query_csource_subscriptions(
                &QueryParams::new().limit(1),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("response should be an array");
        assert_eq!(arr.len(), 1);
    }));

    // 041_02_02: limit=2 (5.11.5)
    trials.push(trial_clean_db(&format!("{PREFIX}::041_02_02::limit_2"), |ctx| async move {
        create_subscriptions(&ctx, 3).await;

        let resp = ctx
            .client
            .query_csource_subscriptions(
                &QueryParams::new().limit(2),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("response should be an array");
        assert_eq!(arr.len(), 2);
    }));

    // 041_02_03: limit=15 (more than exist) (5.11.5)
    trials.push(trial_clean_db(&format!("{PREFIX}::041_02_03::limit_15"), |ctx| async move {
        create_subscriptions(&ctx, 3).await;

        let resp = ctx
            .client
            .query_csource_subscriptions(
                &QueryParams::new().limit(15),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("response should be an array");
        assert_eq!(arr.len(), 3);
    }));

    // ── 041_03: Query with limit and offset ─────────────────────────────

    // 041_03_01: limit=1, offset=1 (5.11.5)
    trials.push(trial_clean_db(&format!("{PREFIX}::041_03_01::limit_1_offset_1"), |ctx| async move {
        create_subscriptions(&ctx, 3).await;

        let resp = ctx
            .client
            .query_csource_subscriptions(
                &QueryParams::new().limit(1).offset(1),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("response should be an array");
        assert_eq!(arr.len(), 1);
    }));

    // 041_03_02: limit=2, offset=2 (5.11.5)
    trials.push(trial_clean_db(&format!("{PREFIX}::041_03_02::limit_2_offset_2"), |ctx| async move {
        create_subscriptions(&ctx, 3).await;

        let resp = ctx
            .client
            .query_csource_subscriptions(
                &QueryParams::new().limit(2).offset(2),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("response should be an array");
        assert_eq!(arr.len(), 1);
    }));

    // 041_03_03: limit=15, offset=0 (all results) (5.11.5)
    trials.push(trial_clean_db(&format!("{PREFIX}::041_03_03::limit_15_offset_0"), |ctx| async move {
        create_subscriptions(&ctx, 3).await;

        let resp = ctx
            .client
            .query_csource_subscriptions(
                &QueryParams::new().limit(15).offset(0),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("response should be an array");
        assert_eq!(arr.len(), 3);
    }));

    // ── 041_04: Invalid limit/offset parameters ─────────────────────────

    // 041_04_01: Negative limit (5.11.5)
    trials.push(trial(&format!("{PREFIX}::041_04_01::invalid_negative_limit"), |ctx| async move {
        let resp = ctx
            .client
            .request(Method::GET, "/csourceSubscriptions?limit=-5&offset=2", None, RequestHeaders::new())
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        assert!(body["title"].is_string(), "response should contain title");
    }));

    // 041_04_02: Negative offset (5.11.5)
    trials.push(trial(&format!("{PREFIX}::041_04_02::invalid_negative_offset"), |ctx| async move {
        let resp = ctx
            .client
            .request(Method::GET, "/csourceSubscriptions?limit=2&offset=-3", None, RequestHeaders::new())
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        assert!(body["title"].is_string(), "response should contain title");
    }));

    // 041_04_03: Zero limit (5.11.5)
    trials.push(trial(&format!("{PREFIX}::041_04_03::invalid_zero_limit"), |ctx| async move {
        let resp = ctx
            .client
            .request(Method::GET, "/csourceSubscriptions?limit=0&offset=0", None, RequestHeaders::new())
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        assert!(body["title"].is_string(), "response should contain title");
    }));
}
