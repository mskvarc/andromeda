use crate::harness::{client::RequestHeaders, fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "context_source::registration::update";

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 034_01: Update endpoint ─────────────────────────────────────────

    // 034_01_01: Update a context source registration by id (5.9.3)
    trials.push(trial(&format!("{PREFIX}::034_01_01::update_endpoint"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:034_01_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("csource_registrations/fragments/context-source-registration-update.json");
        let resp = ctx
            .client
            .request(
                Method::PATCH,
                &format!("/csourceRegistrations/{id}"),
                Some(&fragment),
                RequestHeaders::new().content_type("application/json"),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx.client.retrieve_registration(&id, RequestHeaders::new()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["endpoint"], "http://my.new.csource.org:1026");
    }));

    // ── 034_02: Update error cases ──────────────────────────────────────

    // 034_02_01: Invalid URI (5.9.3)
    trials.push(trial(&format!("{PREFIX}::034_02_01::invalid_uri"), |ctx| async move {
        let fragment = fixtures::load_fixture_original_context(
            "csource_registrations/fragments/context-source-registration-different-type.jsonld",
            "invalidURI",
        );

        let resp = ctx
            .client
            .request(
                Method::PATCH,
                "/csourceRegistrations/invalidURI",
                Some(&fragment),
                RequestHeaders::new().content_type("application/ld+json"),
            )
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
    }));

    // 034_02_02: Request body not same data type (5.9.3)
    trials.push(trial(&format!("{PREFIX}::034_02_02::wrong_data_type"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:034_02_02:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fixture_original_context("csource_registrations/fragments/context-source-registration-different-type.jsonld", &id);
        let resp = ctx
            .client
            .request(
                Method::PATCH,
                &format!("/csourceRegistrations/{id}"),
                Some(&fragment),
                RequestHeaders::new().content_type("application/ld+json"),
            )
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
    }));

    // 034_02_03: Attempt to remove mandatory property (5.9.3)
    trials.push(trial(&format!("{PREFIX}::034_02_03::remove_mandatory_property"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:034_02_03:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-invalid.jsonld", &id);
        let resp = ctx
            .client
            .request(
                Method::PATCH,
                &format!("/csourceRegistrations/{id}"),
                Some(&fragment),
                RequestHeaders::new().content_type("application/ld+json"),
            )
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
    }));

    // ── 034_03: Update unknown registration ─────────────────────────────

    // 034_03_01: ID not known to the system (5.9.3)
    trials.push(trial(&format!("{PREFIX}::034_03_01::unknown_registration"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:034_03_01:{}", uuid::Uuid::new_v4());
        let fragment = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.jsonld", &id);

        let resp = ctx
            .client
            .request(
                Method::PATCH,
                &format!("/csourceRegistrations/{id}"),
                Some(&fragment),
                RequestHeaders::new().content_type("application/ld+json"),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound");
    }));

    // ── 034_04: Invalid request body ────────────────────────────────────

    // 034_04_01: Malformed JSON (5.9.3)
    trials.push(trial(&format!("{PREFIX}::034_04_01::invalid_request_body"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:034_04_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let raw = fixtures::load_fixture_raw("csource_registrations/context-source-registration-invalid-json.json");
        let resp = ctx
            .client
            .request_raw(Method::PATCH, &format!("/csourceRegistrations/{id}"), raw.as_bytes(), "application/json")
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest");
        assert!(body["title"].is_string(), "response should contain title");
    }));

    // ── 034_05: Update to never expire ──────────────────────────────────

    // 034_05_01: Set expiresAt to null (5.9.3)
    trials.push(trial(&format!("{PREFIX}::034_05_01::update_to_never_expire"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:034_05_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration-with-expiration.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("csource_registrations/fragments/context-source-registration-null-expiresAt.json");
        let resp = ctx
            .client
            .request(
                Method::PATCH,
                &format!("/csourceRegistrations/{id}"),
                Some(&fragment),
                RequestHeaders::new().content_type("application/json"),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx.client.retrieve_registration(&id, RequestHeaders::new()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert!(body.get("expiresAt").is_none(), "expiresAt should be absent after setting to null");
    }));

    // ── 034_06: Update without ID → 405 ─────────────────────────────────

    // 034_06_01: PATCH /csourceRegistrations/ with no ID in URL (5.9.3)
    trials.push(trial(&format!("{PREFIX}::034_06_01::update_without_id"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:034_06_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let mut fragment = fixtures::load_fragment("csource_registrations/fragments/context-source-registration-different-type.jsonld");
        fragment.as_object_mut().unwrap().remove("id");
        let resp = ctx
            .client
            .request(
                Method::PATCH,
                "/csourceRegistrations/",
                Some(&fragment),
                RequestHeaders::new().content_type("application/ld+json"),
            )
            .await;
        assert_eq!(resp.status, StatusCode::METHOD_NOT_ALLOWED);
    }));
}
