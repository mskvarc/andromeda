use crate::{
    distributed_operations::prepare_registration,
    harness::{
        client::{QueryParams, RequestHeaders},
        fixtures,
        mock_server::MockServer,
        trial::trial,
    },
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "distributed_operations::consumption::entity::warning_headers";

const VEHICLE_FIXTURE: &str = "entities/vehicle-simple-attributes.json";
const VEHICLE_FIXTURE_LD: &str = "entities/vehicle-simple-attributes.jsonld";
const REG_VEHICLE_COMPLETE: &str = "csource_registrations/context-source-registration-vehicle-complete.jsonld";

fn ld_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

/// Collect all NGSILD-Warning header values from a response.
fn warning_header_values(resp: &crate::harness::client::NgsiLdResponse) -> Vec<String> {
    resp.headers
        .get_all("NGSILD-Warning")
        .iter()
        .filter_map(|v| v.to_str().ok().map(|s| s.to_string()))
        .collect()
}

pub fn tests(trials: &mut Vec<Trial>) {
    // D_WARN_01 -- Query: context source returns HTTP 500 => warning code 299
    trials.push(trial(
        &format!("{PREFIX}::D_WARN_01::query_entities_context_source_error_returns_warning_299"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D_WARN_01");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D_WARN_01");

            let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None);
            let resp = ctx.client.create_registration(&reg).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            mock.set_stub(Method::GET, "/ngsi-ld/v1/entities", 500).await;

            let resp = ctx.client.query_entities(&QueryParams::new().type_("Vehicle"), ld_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let warnings = warning_header_values(&resp);
            assert!(!warnings.is_empty(), "expected NGSILD-Warning header, got none");
            assert!(warnings.iter().any(|w| w.starts_with("299")), "expected warning code 299, got: {warnings:?}");

            mock.stop();
        },
    ));

    // D_WARN_02 -- Query: context source unreachable => warning code 199
    trials.push(trial(
        &format!("{PREFIX}::D_WARN_02::query_entities_context_source_timeout_returns_warning_199"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D_WARN_02");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D_WARN_02");

            // Use the mock's URL but stop it so it's unreachable
            let endpoint = mock.url("");
            mock.stop();

            let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &endpoint, None, None);
            let resp = ctx.client.create_registration(&reg).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            let resp = ctx.client.query_entities(&QueryParams::new().type_("Vehicle"), ld_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let warnings = warning_header_values(&resp);
            assert!(!warnings.is_empty(), "expected NGSILD-Warning header, got none");
            assert!(warnings.iter().any(|w| w.starts_with("199")), "expected warning code 199, got: {warnings:?}");
        },
    ));

    // D_WARN_03 -- Query: context source returns invalid (non-JSON) payload => warning code 111
    trials.push(trial(
        &format!("{PREFIX}::D_WARN_03::query_entities_context_source_invalid_payload_returns_warning_111"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D_WARN_03");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D_WARN_03");

            let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None);
            let resp = ctx.client.create_registration(&reg).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            mock.set_stub_with_raw_body(Method::GET, "/ngsi-ld/v1/entities", 200, "not json at all", "text/plain")
                .await;

            let resp = ctx.client.query_entities(&QueryParams::new().type_("Vehicle"), ld_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let warnings = warning_header_values(&resp);
            assert!(!warnings.is_empty(), "expected NGSILD-Warning header, got none");
            assert!(warnings.iter().any(|w| w.starts_with("111")), "expected warning code 111, got: {warnings:?}");

            mock.stop();
        },
    ));

    // D_WARN_04 -- Query: context source returns 404 => no warning (spec-compliant)
    trials.push(trial(
        &format!("{PREFIX}::D_WARN_04::query_entities_context_source_404_no_warning"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D_WARN_04");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D_WARN_04");

            let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None);
            let resp = ctx.client.create_registration(&reg).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            mock.set_stub(Method::GET, "/ngsi-ld/v1/entities", 404).await;

            let resp = ctx.client.query_entities(&QueryParams::new().type_("Vehicle"), ld_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let warnings = warning_header_values(&resp);
            assert!(warnings.is_empty(), "expected no NGSILD-Warning header for 404, got: {warnings:?}");

            mock.stop();
        },
    ));

    // D_WARN_05 -- Retrieve: context source returns HTTP 403 => warning code 299
    trials.push(trial(
        &format!("{PREFIX}::D_WARN_05::retrieve_entity_context_source_error_returns_warning_299"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D_WARN_05");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D_WARN_05");

            // Create local entity so we get 200 (not 404)
            let entity = fixtures::load_fixture(VEHICLE_FIXTURE_LD, &entity_id);
            let resp = ctx
                .client
                .request(Method::POST, "/entities?local=true", Some(&entity), RequestHeaders::new())
                .await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None);
            let resp = ctx.client.create_registration(&reg).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            mock.set_stub(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 403).await;

            let resp = ctx.client.get_entity(&entity_id, &QueryParams::new(), ld_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let warnings = warning_header_values(&resp);
            assert!(!warnings.is_empty(), "expected NGSILD-Warning header, got none");
            assert!(warnings.iter().any(|w| w.starts_with("299")), "expected warning code 299, got: {warnings:?}");

            mock.stop();
        },
    ));

    // D_WARN_06 -- Query: successful forwarding => no warning header
    trials.push(trial(
        &format!("{PREFIX}::D_WARN_06::query_entities_successful_forwarding_no_warning"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D_WARN_06");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D_WARN_06");

            let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None);
            let resp = ctx.client.create_registration(&reg).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
            let mock_array = serde_json::Value::Array(vec![mock_entity]);
            mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

            let resp = ctx.client.query_entities(&QueryParams::new().type_("Vehicle"), ld_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let warnings = warning_header_values(&resp);
            assert!(
                warnings.is_empty(),
                "expected no NGSILD-Warning header for successful forwarding, got: {warnings:?}"
            );

            mock.stop();
        },
    ));

    // D_WARN_07 -- Query: multiple context sources with mixed results => multiple warnings
    trials.push(trial(
        &format!("{PREFIX}::D_WARN_07::query_entities_multiple_sources_multiple_warnings"),
        |ctx| async move {
            let mut mock1 = MockServer::start().await;
            let mut mock2 = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D_WARN_07");
            let reg_id1 = ctx.entity_id("urn:ngsi-ld:Registration:D_WARN_07_1");
            let reg_id2 = ctx.entity_id("urn:ngsi-ld:Registration:D_WARN_07_2");

            // First CSR: mock returns 500
            let reg1 = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id1, &entity_id, &mock1.url(""), None, None);
            let resp = ctx.client.create_registration(&reg1).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id1);

            mock1.set_stub(Method::GET, "/ngsi-ld/v1/entities", 500).await;

            // Second CSR: mock stopped (unreachable)
            let endpoint2 = mock2.url("");
            mock2.stop();

            let reg2 = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id2, &entity_id, &endpoint2, None, None);
            let resp = ctx.client.create_registration(&reg2).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id2);

            let resp = ctx.client.query_entities(&QueryParams::new().type_("Vehicle"), ld_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let warnings = warning_header_values(&resp);
            assert!(
                warnings.len() >= 2,
                "expected at least 2 NGSILD-Warning headers, got {}: {warnings:?}",
                warnings.len()
            );

            mock1.stop();
        },
    ));

    // D_WARN_08 -- Query with local=true => no forwarding, no warnings
    trials.push(trial(
        &format!("{PREFIX}::D_WARN_08::query_entities_local_true_no_warnings"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D_WARN_08");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D_WARN_08");

            // Create local entity
            let entity = fixtures::load_fixture(VEHICLE_FIXTURE_LD, &entity_id);
            let resp = ctx
                .client
                .request(Method::POST, "/entities?local=true", Some(&entity), RequestHeaders::new())
                .await;
            assert_eq!(resp.status, StatusCode::CREATED);

            // CSR points to mock returning 500 -- but local=true should skip forwarding
            let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None);
            let resp = ctx.client.create_registration(&reg).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            mock.set_stub(Method::GET, "/ngsi-ld/v1/entities", 500).await;

            let resp = ctx.client.query_entities(&QueryParams::new().type_("Vehicle").local(true), ld_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let warnings = warning_header_values(&resp);
            assert!(warnings.is_empty(), "expected no NGSILD-Warning header with local=true, got: {warnings:?}");

            mock.stop();
        },
    ));
}
