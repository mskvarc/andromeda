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

const PREFIX: &str = "distributed_operations::consumption::temporal::retrieve_temporal";
const TEMPORAL_VEHICLE_FIXTURE: &str = "temporal_entities/2020-08-vehicle-temporal-representation.jsonld";
const REG_VEHICLE_COMPLETE: &str = "csource_registrations/context-source-registration-vehicle-complete.jsonld";

fn ld_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

fn warning_header_values(resp: &crate::harness::client::NgsiLdResponse) -> Vec<String> {
    resp.headers
        .get_all("NGSILD-Warning")
        .iter()
        .filter_map(|v| v.to_str().ok().map(|s| s.to_string()))
        .collect()
}

pub fn tests(trials: &mut Vec<Trial>) {
    trials.push(trial(&format!("{PREFIX}::retrieve_temporal_local_only"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:tr_local");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:tr_local");

        // Create local temporal entity
        let entity = fixtures::load_fixture(TEMPORAL_VEHICLE_FIXTURE, &entity_id);
        let resp = ctx.client.create_temporal_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_temporal_entity(&entity_id);

        // Create CSR
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Stub the retrieve endpoint
        let mock_entity = fixtures::load_fixture(TEMPORAL_VEHICLE_FIXTURE, &entity_id);
        mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}"), 200, &mock_entity)
            .await;

        // Retrieve with local=true
        let resp = ctx.client.get_temporal_entity(&entity_id, &QueryParams::new().local(true), ld_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        assert_eq!(
            mock.get_count(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}")).await,
            0,
            "mock should not be called when local=true"
        );

        mock.stop();
    }));

    trials.push(trial(&format!("{PREFIX}::retrieve_temporal_merge_remote"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:tr_merge");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:tr_merge");

        // Create local temporal entity (has speed, fuelLevel)
        let entity = fixtures::load_fixture(TEMPORAL_VEHICLE_FIXTURE, &entity_id);
        let resp = ctx.client.create_temporal_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_temporal_entity(&entity_id);

        // Create CSR
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns same entity with an extra attribute
        let mock_entity = serde_json::json!({
            "id": entity_id,
            "type": "Vehicle",
            "altitude": [{"type": "Property", "value": 500, "observedAt": "2020-08-01T12:00:00Z"}]
        });
        mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}"), 200, &mock_entity)
            .await;

        let resp = ctx.client.get_temporal_entity(&entity_id, &QueryParams::new(), ld_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json_value();
        // Should have both local (speed, fuelLevel) and remote (altitude) attributes
        assert!(body.get("speed").is_some(), "local attribute 'speed' should be present");
        assert!(body.get("altitude").is_some(), "remote attribute 'altitude' should be merged");

        mock.stop();
    }));

    trials.push(trial(&format!("{PREFIX}::retrieve_temporal_only_remote"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:tr_remote_only");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:tr_remote_only");

        // No local entity created -- only exists in remote
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        let mock_entity = serde_json::json!({
            "id": entity_id,
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": 42, "observedAt": "2020-08-01T12:00:00Z"}]
        });
        mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}"), 200, &mock_entity)
            .await;

        let resp = ctx.client.get_temporal_entity(&entity_id, &QueryParams::new(), ld_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json_value();
        assert_eq!(body.get("id").and_then(|v| v.as_str()), Some(entity_id.as_str()));
        assert!(body.get("speed").is_some(), "remote entity speed should be present");

        mock.stop();
    }));

    trials.push(trial(&format!("{PREFIX}::retrieve_temporal_warning_on_error"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:tr_warn");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:tr_warn");

        // Create local temporal entity so we get a 200 even with CS error
        let entity = fixtures::load_fixture(TEMPORAL_VEHICLE_FIXTURE, &entity_id);
        let resp = ctx.client.create_temporal_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_temporal_entity(&entity_id);

        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        mock.set_stub(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{entity_id}"), 500).await;

        let resp = ctx.client.get_temporal_entity(&entity_id, &QueryParams::new(), ld_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let warnings = warning_header_values(&resp);
        assert!(!warnings.is_empty(), "expected NGSILD-Warning header on CS error, got none");
        assert!(warnings.iter().any(|w| w.starts_with("299")), "expected warning code 299, got: {warnings:?}");

        mock.stop();
    }));
}
