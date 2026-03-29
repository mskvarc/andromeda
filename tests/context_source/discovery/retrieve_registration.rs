use crate::harness::{client::RequestHeaders, fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

const PREFIX: &str = "context_source::discovery::retrieve";

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
    // ── 036_01: Retrieve with invalid ID ──────────────────────────────

    // 036_01_01: Invalid URI → 400 (5.10.1)
    trials.push(trial(&format!("{PREFIX}::036_01_01::invalid_id"), |ctx| async move {
        let resp = ctx.client.retrieve_registration("invalidUri", RequestHeaders::new()).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid ID should return 400");

        let body: serde_json::Value = resp.json();
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
            "error type should be BadRequestData"
        );
    }));

    // ── 036_02: Retrieve unknown registration ─────────────────────────

    // 036_02_01: Unknown registration → 404 (5.10.1)
    trials.push(trial(&format!("{PREFIX}::036_02_01::unknown_registration"), |ctx| async move {
        let resp = ctx
            .client
            .retrieve_registration("urn:ngsi-ld:ContextSourceRegistration:unknowRegistration", RequestHeaders::new())
            .await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND, "unknown registration should return 404");

        let body: serde_json::Value = resp.json();
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound",
            "error type should be ResourceNotFound"
        );
    }));

    // ── 036_03: Retrieve with default core context ────────────────────

    // 036_03_01: Retrieve without context → expanded format (5.10.1)
    // Created with test suite context so terms are expanded to FQNs during creation.
    // Retrieved without Link header → Core @context used for compaction →
    // custom terms not in Core → rendered as FQNs (spec 5.5.7).
    trials.push(trial(&format!("{PREFIX}::036_03_01::default_core_context"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:036_03_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.retrieve_registration(&id, RequestHeaders::new()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "createdAt", "modifiedAt", "status"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("csource_registrations/expectations/context-source-registration-expanded-format.json");
        expected["id"] = serde_json::Value::String(id.clone());
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // ── 036_04: Retrieve with Link context ────────────────────────────

    // 036_04_01: Retrieve with context → compacted format (5.10.1)
    // Created with test suite context, retrieved with same context as Link header →
    // FQNs compacted back to short names (spec 5.5.7).
    trials.push(trial(&format!("{PREFIX}::036_04_01::with_context"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:036_04_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .retrieve_registration(&id, RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "createdAt", "modifiedAt", "status"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("csource_registrations/expectations/context-source-registration.json");
        expected["id"] = serde_json::Value::String(id.clone());
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // ── 036_05: JSON-LD context resolution ────────────────────────────

    // 036_05_01: Empty JSON-LD context → expanded format (6.3.5)
    // Same as 036_03_01: no Link header → Core @context → FQNs rendered.
    trials.push(trial(&format!("{PREFIX}::036_05_01::empty_jsonld_context"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:036_05_01:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.retrieve_registration(&id, RequestHeaders::new()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "createdAt", "modifiedAt", "status"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("csource_registrations/expectations/context-source-registration-expanded-format.json");
        expected["id"] = serde_json::Value::String(id.clone());
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));

    // 036_05_02: Creation-time JSON-LD context → compacted format (6.3.5)
    // Same as 036_04_01: Link header with test suite context → compacted names.
    trials.push(trial(&format!("{PREFIX}::036_05_02::creation_time_context"), |ctx| async move {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:036_05_02:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&id);
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.jsonld", &id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .retrieve_registration(&id, RequestHeaders::new().accept_ld_json().link_context(fixtures::compound_context_url()))
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let ignored = &["@context", "createdAt", "modifiedAt", "status"];
        let mut actual: serde_json::Value = resp.json();
        let mut expected = fixtures::load_expectation("csource_registrations/expectations/context-source-registration.json");
        expected["id"] = serde_json::Value::String(id.clone());
        strip_ignored_keys(&mut actual, ignored);
        strip_ignored_keys(&mut expected, ignored);
        assert_eq!(expected, actual);
    }));
}
