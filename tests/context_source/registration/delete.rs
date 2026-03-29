use crate::harness::{client::RequestHeaders, fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "context_source::registration::delete";

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 035_01: Delete by ID ────────────────────────────────────────────

    // 035_01_01: Delete a context source registration by id (5.9.4)
    trials.push(trial(&format!("{PREFIX}::035_01_01::delete_by_id"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:035_01_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_registration(&id).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx.client.retrieve_registration(&id, RequestHeaders::new()).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // ── 035_02: Delete error cases ──────────────────────────────────────

    // 035_02_01: ID not present (empty path) (5.9.4)
    trials.push(trial(&format!("{PREFIX}::035_02_01::id_not_present"), |ctx| async move {
        let resp = ctx.client.request(Method::DELETE, "/csourceRegistrations/", None, RequestHeaders::new()).await;
        assert_eq!(resp.status, StatusCode::METHOD_NOT_ALLOWED);
    }));

    // 035_02_02: Invalid URI (5.9.4)
    trials.push(trial(&format!("{PREFIX}::035_02_02::invalid_uri"), |ctx| async move {
        let resp = ctx.client.delete_registration("invalidURI").await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        assert!(body["title"].is_string(), "response should contain title");
    }));

    // ── 035_03: Delete unknown registration ─────────────────────────────

    // 035_03_01: ID not known to the system (5.9.4)
    trials.push(trial(&format!("{PREFIX}::035_03_01::unknown_registration"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:035_03_01:{}", uuid::Uuid::new_v4());
        let resp = ctx.client.delete_registration(&id).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);

        let body: serde_json::Value = resp.json();
        assert!(body["title"].is_string(), "response should contain title");
    }));
}
