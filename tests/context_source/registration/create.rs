use crate::harness::{client::RequestHeaders, fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "context_source::registration::create";

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
    // ── 033_01: Basic creation ──────────────────────────────────────────

    // 033_01_01: Create without mode and operation (5.9.2)
    trials.push(trial(&format!("{PREFIX}::033_01_01::without_mode_and_operation"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:033_01_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let location = resp.location().expect("Location header should be present");
        assert!(location.contains(&id), "Location header should contain the registration ID");

        let resp = ctx
            .client
            .retrieve_registration(&id, RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "createdAt", "modifiedAt", "status", "mode", "operations"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = reg;
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // 033_01_02: Create with non-default mode and operations (5.9.2)
    trials.push(trial(&format!("{PREFIX}::033_01_02::with_mode_and_operations"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:033_01_02:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-with-mode-and-operations.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .retrieve_registration(&id, RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "createdAt", "modifiedAt", "status"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = reg;
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // 033_01_03: Create with location (5.9.2)
    trials.push(trial(&format!("{PREFIX}::033_01_03::with_location"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:033_01_03:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-location.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .retrieve_registration(&id, RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "createdAt", "modifiedAt", "status", "mode", "operations"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = reg;
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // 033_01_04: Create with specific expiration date (5.9.2)
    trials.push(trial(&format!("{PREFIX}::033_01_04::with_expiration"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:033_01_04:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-with-expiration.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .retrieve_registration(&id, RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "createdAt", "modifiedAt", "status", "mode", "operations"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = reg;
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // ── 033_02: Invalid content ─────────────────────────────────────────

    // 033_02_01: Create with invalid content (information is null) (5.9.2)
    trials.push(trial(&format!("{PREFIX}::033_02_01::invalid_content"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:033_02_01:{}", uuid::Uuid::new_v4());
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-invalid.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        assert!(body["title"].is_string(), "response should contain title");
    }));

    // ── 033_03: Duplicate registration ──────────────────────────────────

    // 033_03_01: Create registration that already exists (5.9.2)
    trials.push(trial(&format!("{PREFIX}::033_03_01::already_exists"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:033_03_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CONFLICT);

        let body: serde_json::Value = resp.json();
        assert!(body["title"].is_string(), "response should contain title");
    }));

    // ── 033_04: JSON content with Link header context ───────────────────

    // 033_04_01: With Link header context, retrieve with context (6.3.5)
    trials.push(trial(&format!("{PREFIX}::033_04_01::json_with_link_context"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:033_04_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let mut reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.json", &id);
        // Remove @context if present (plain JSON should not have it)
        reg.as_object_mut().unwrap().remove("@context");

        let resp = ctx
            .client
            .request(
                Method::POST,
                "/csourceRegistrations/",
                Some(&reg),
                RequestHeaders::new()
                    .content_type("application/json")
                    .link_context(fixtures::compound_context_url()),
            )
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .retrieve_registration(&id, RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["information"][0]["entities"][0]["type"], "Building");
    }));

    // 033_04_02: With Link header context, retrieve without context (6.3.5)
    trials.push(trial(
        &format!("{PREFIX}::033_04_02::json_with_link_context_retrieve_without"),
        |ctx| async move {
            let id = format!("urn:ngsi-ld:ContextSourceRegistration:033_04_02:{}", uuid::Uuid::new_v4());
            ctx.track_registration(&id);
            let mut reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.json", &id);
            reg.as_object_mut().unwrap().remove("@context");

            let resp = ctx
                .client
                .request(
                    Method::POST,
                    "/csourceRegistrations/",
                    Some(&reg),
                    RequestHeaders::new()
                        .content_type("application/json")
                        .link_context(fixtures::compound_context_url()),
                )
                .await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let resp = ctx.client.retrieve_registration(&id, RequestHeaders::new()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let body: serde_json::Value = resp.json();
            assert_eq!(body["information"][0]["entities"][0]["type"], "https://ngsi-ld-test-suite/context#Building");
        },
    ));

    // ── 033_05: Default context with JSON content ───────────────────────

    // 033_05_01: Default context, retrieve with test suite context (6.3.5)
    trials.push(trial(
        &format!("{PREFIX}::033_05_01::default_context_retrieve_with_context"),
        |ctx| async move {
            let id = format!("urn:ngsi-ld:ContextSourceRegistration:033_05_01:{}", uuid::Uuid::new_v4());
            ctx.track_registration(&id);
            let mut reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.json", &id);
            reg.as_object_mut().unwrap().remove("@context");

            let resp = ctx
                .client
                .request(
                    Method::POST,
                    "/csourceRegistrations/",
                    Some(&reg),
                    RequestHeaders::new().content_type("application/json"),
                )
                .await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let resp = ctx
                .client
                .retrieve_registration(&id, RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()))
                .await;
            assert_eq!(resp.status, StatusCode::OK);

            // Created with default context (core @context per spec 5.5.5). The core @context's
            // @vocab expands "Building" to "ngsi-ld:default-context/Building". The compound
            // context can't compact this back since it maps "Building" to a different URI.
            let body: serde_json::Value = resp.json();
            assert_eq!(body["information"][0]["entities"][0]["type"], "ngsi-ld:default-context/Building");
        },
    ));

    // 033_05_02: Default context, retrieve without context (6.3.5)
    trials.push(trial(
        &format!("{PREFIX}::033_05_02::default_context_retrieve_without_context"),
        |ctx| async move {
            let id = format!("urn:ngsi-ld:ContextSourceRegistration:033_05_02:{}", uuid::Uuid::new_v4());
            ctx.track_registration(&id);
            let mut reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.json", &id);
            reg.as_object_mut().unwrap().remove("@context");

            let resp = ctx
                .client
                .request(
                    Method::POST,
                    "/csourceRegistrations/",
                    Some(&reg),
                    RequestHeaders::new().content_type("application/json"),
                )
                .await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let resp = ctx.client.retrieve_registration(&id, RequestHeaders::new()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let body: serde_json::Value = resp.json();
            assert_eq!(body["information"][0]["entities"][0]["type"], "Building");
        },
    ));

    // ── 033_06: JSON content type with @context in body ─────────────────

    // 033_06_01: @context in body with application/json content type (6.3.5)
    trials.push(trial(
        &format!("{PREFIX}::033_06_01::jsonld_context_with_json_content_type"),
        |ctx| async move {
            let id = format!("urn:ngsi-ld:ContextSourceRegistration:033_06_01:{}", uuid::Uuid::new_v4());
            let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.jsonld", &id);

            let resp = ctx
                .client
                .request(
                    Method::POST,
                    "/csourceRegistrations/",
                    Some(&reg),
                    RequestHeaders::new().content_type("application/json"),
                )
                .await;
            assert_eq!(resp.status, StatusCode::BAD_REQUEST);

            let body: serde_json::Value = resp.json();
            assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
            assert!(body["title"].is_string(), "response should contain title");
        },
    ));

    // ── 033_07: JSON-LD content type ────────────────────────────────────

    // 033_07_01: JSON-LD @context from request payload, retrieve with context (6.3.5)
    trials.push(trial(
        &format!("{PREFIX}::033_07_01::jsonld_context_from_payload_with_context"),
        |ctx| async move {
            let id = format!("urn:ngsi-ld:ContextSourceRegistration:033_07_01:{}", uuid::Uuid::new_v4());
            ctx.track_registration(&id);
            let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.jsonld", &id);

            let resp = ctx.client.create_registration(&reg).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let resp = ctx
                .client
                .retrieve_registration(&id, RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()))
                .await;
            assert_eq!(resp.status, StatusCode::OK);

            let body: serde_json::Value = resp.json();
            assert_eq!(body["information"][0]["entities"][0]["type"], "Vehicle");
        },
    ));

    // 033_07_02: JSON-LD @context from request payload, retrieve without context (6.3.5)
    trials.push(trial(
        &format!("{PREFIX}::033_07_02::jsonld_context_from_payload_without_context"),
        |ctx| async move {
            let id = format!("urn:ngsi-ld:ContextSourceRegistration:033_07_02:{}", uuid::Uuid::new_v4());
            ctx.track_registration(&id);
            let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.jsonld", &id);

            let resp = ctx.client.create_registration(&reg).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let resp = ctx.client.retrieve_registration(&id, RequestHeaders::new()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let body: serde_json::Value = resp.json();
            assert_eq!(body["information"][0]["entities"][0]["type"], "https://ngsi-ld-test-suite/context#Vehicle");
        },
    ));

    // ── 033_08: JSON-LD content type without @context ───────────────────

    // 033_08_01: No @context in body with application/ld+json content type (6.3.5)
    trials.push(trial(&format!("{PREFIX}::033_08_01::no_context_with_jsonld_content_type"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:033_08_01:{}", uuid::Uuid::new_v4());
        let mut reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.json", &id);
        reg.as_object_mut().unwrap().remove("@context");

        let resp = ctx
            .client
            .request(
                Method::POST,
                "/csourceRegistrations/",
                Some(&reg),
                RequestHeaders::new().content_type("application/ld+json"),
            )
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        assert!(body["title"].is_string(), "response should contain title");
    }));

    // ── 033_09: JSON-LD content type with Link header ───────────────────

    // 033_09_01: Link header AND JSON-LD content type (6.3.5)
    trials.push(trial(&format!("{PREFIX}::033_09_01::link_header_with_jsonld_content_type"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:033_09_01:{}", uuid::Uuid::new_v4());
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.jsonld", &id);

        let resp = ctx
            .client
            .request(
                Method::POST,
                "/csourceRegistrations/",
                Some(&reg),
                RequestHeaders::new()
                    .content_type("application/ld+json")
                    .link_context(fixtures::test_suite_context_url()),
            )
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        assert!(body["title"].is_string(), "response should contain title");
    }));

    // ── 033_10: Invalid structure / past expiration ─────────────────────

    // 033_10_01: Different data structure (missing information field) (5.9.2)
    trials.push(trial(&format!("{PREFIX}::033_10_01::invalid_structure"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:033_10_01:{}", uuid::Uuid::new_v4());
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-invalid-structure.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // 033_10_02: Expiration date in the past (5.9.2)
    trials.push(trial(&format!("{PREFIX}::033_10_02::past_expiration"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:033_10_02:{}", uuid::Uuid::new_v4());
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-past-expiration.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // ── 033_11: Create without specifying an ID ─────────────────────────

    // 033_11: Server generates ID (5.9.2)
    trials.push(trial(&format!("{PREFIX}::033_11::without_id"), |ctx| async move {
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-no-id.jsonld", "unused");

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let location = resp.location().expect("response should contain Location header").to_string();
        assert!(!location.is_empty(), "Location header should not be empty");
        // Location header is the full relative path; extract just the ID (last path segment)
        let id = location.rsplit('/').next().expect("Location should contain an ID");
        ctx.track_registration(id);

        let resp = ctx
            .client
            .retrieve_registration(id, RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert!(body["id"].is_string(), "response should contain id");
    }));
}
