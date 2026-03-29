use crate::harness::{client::RequestHeaders, fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

const PREFIX: &str = "context_source::registration_subscription::create";

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 038_01: Basic creation ──────────────────────────────────────────

    // 038_01_01: Create a minimal context source registration subscription (5.11.2)
    trials.push(trial(&format!("{PREFIX}::038_01_01::basic_creation"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:038_01_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&id);
        let sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &id);

        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let location = resp.location().expect("Location header should be present");
        assert!(location.contains(&id), "Location header should contain the subscription ID");
    }));

    // ── 038_02: Create without an ID ────────────────────────────────────

    // 038_02_01: Create without providing an ID — server generates one (5.11.2)
    trials.push(trial(&format!("{PREFIX}::038_02_01::without_id"), |ctx| async move {
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", "unused");
        sub.as_object_mut().unwrap().remove("id");

        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let location = resp.location().expect("Location header should be present").to_string();
        assert!(!location.is_empty(), "Location header should not be empty");
        let id = location.rsplit('/').next().expect("Location should contain an ID");
        ctx.track_csource_subscription(id);
    }));

    // ── 038_03: Default isActive ────────────────────────────────────────

    // 038_03_01: Without isActive member, subscription should be active by default (5.11.2)
    trials.push(trial(&format!("{PREFIX}::038_03_01::default_is_active"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:038_03_01:{}", uuid::Uuid::new_v4());
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
        assert_eq!(body["status"], "active");
    }));

    // ── 038_04: Create inactive subscription ────────────────────────────

    // 038_04_01: With isActive set to false, status should be "paused" (5.11.2)
    trials.push(trial(&format!("{PREFIX}::038_04_01::inactive"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:038_04_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&id);
        let sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription-inactive.jsonld", &id);

        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .retrieve_csource_subscription(&id, RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["isActive"], false);
        assert_eq!(body["status"], "paused");
    }));

    // ── 038_05: Create with expiresAt ───────────────────────────────────

    // 038_05_01: With expiresAt, status should change to "expired" when due (5.11.2)
    trials.push(trial(&format!("{PREFIX}::038_05_01::with_expires_at"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:038_05_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&id);
        let mut sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription-expiresAt.jsonld", &id);

        // Set expiresAt to 3 seconds from now
        let expires = chrono::Utc::now() + chrono::Duration::seconds(3);
        sub["expiresAt"] = serde_json::Value::String(expires.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));

        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Wait for expiration
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        let resp = ctx
            .client
            .retrieve_csource_subscription(&id, RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["status"], "expired");
    }));

    // ── 038_06: Create without expiresAt ────────────────────────────────

    // 038_06_01: Without expiresAt, subscription should remain active (perpetual) (5.11.2)
    trials.push(trial(&format!("{PREFIX}::038_06_01::without_expires_at"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:038_06_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&id);
        let sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &id);

        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Wait and verify subscription is still active
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        let resp = ctx
            .client
            .retrieve_csource_subscription(&id, RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["status"], "active");
    }));

    // ── 038_07: Create duplicate subscription ───────────────────────────

    // 038_07_01: Creating a subscription with an already-existing ID returns 409 (5.11.2)
    trials.push(trial(&format!("{PREFIX}::038_07_01::already_exists"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:038_07_01:{}", uuid::Uuid::new_v4());
        ctx.track_csource_subscription(&id);
        let sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription.jsonld", &id);

        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CONFLICT);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/AlreadyExists");
        assert!(body["title"].is_string(), "response should contain title");
    }));

    // ── 038_08: Invalid subscriptions ───────────────────────────────────

    // 038_08_01: Without notification member (5.11.2)
    trials.push(trial(&format!("{PREFIX}::038_08_01::without_notification"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:038_08_01:{}", uuid::Uuid::new_v4());
        let sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription-without-notification.jsonld", &id);

        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        assert!(body["title"].is_string(), "response should contain title");
    }));

    // 038_08_02: Invalid type (5.11.2)
    trials.push(trial(&format!("{PREFIX}::038_08_02::invalid_type"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:038_08_02:{}", uuid::Uuid::new_v4());
        let sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription-invalid-type.jsonld", &id);

        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        assert!(body["title"].is_string(), "response should contain title");
    }));

    // 038_08_03: Invalid query (5.11.2)
    trials.push(trial(&format!("{PREFIX}::038_08_03::invalid_query"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:038_08_03:{}", uuid::Uuid::new_v4());
        let sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription-invalid-query.jsonld", &id);

        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        assert!(body["title"].is_string(), "response should contain title");
    }));

    // 038_08_04: Empty watchedAttributes (5.11.2)
    trials.push(trial(&format!("{PREFIX}::038_08_04::empty_watched_attributes"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:038_08_04:{}", uuid::Uuid::new_v4());
        let sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription-empty-watchedAttributes.jsonld", &id);

        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        assert!(body["title"].is_string(), "response should contain title");
    }));

    // ── 038_09: Create with past expiresAt ──────────────────────────────

    // 038_09_01: expiresAt in the past should be rejected (5.11.2)
    trials.push(trial(&format!("{PREFIX}::038_09_01::expired"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Subscription:038_09_01:{}", uuid::Uuid::new_v4());
        let sub = fixtures::load_fixture_original_context("csource_subscriptions/subscription-expired.jsonld", &id);

        let resp = ctx.client.create_csource_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        assert!(body["title"].is_string(), "response should contain title");
    }));
}
