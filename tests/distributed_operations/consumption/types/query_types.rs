use crate::{
    distributed_operations::prepare_registration,
    harness::{client::QueryParams, fixtures, mock_server::MockServer, trial::trial},
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "distributed_operations::consumption::types::query_types";
const VEHICLE_FIXTURE_LD: &str = "entities/vehicle-simple-attributes.jsonld";
const REG_VEHICLE_COMPLETE: &str = "csource_registrations/context-source-registration-vehicle-complete.jsonld";

fn warning_header_values(resp: &crate::harness::client::NgsiLdResponse) -> Vec<String> {
    resp.headers
        .get_all("NGSILD-Warning")
        .iter()
        .filter_map(|v| v.to_str().ok().map(|s| s.to_string()))
        .collect()
}

pub fn tests(trials: &mut Vec<Trial>) {
    trials.push(trial(&format!("{PREFIX}::query_types_local_skips_forwarding"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:types_local_skip");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:types_local_skip");

        // Create a CSR pointing to mock
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Stub the types endpoint
        let mock_types = serde_json::json!({"id": "urn:ngsi-ld:EntityTypeList:mock", "type": "EntityTypeList", "typeList": ["Vehicle"]});
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/types", 200, &mock_types).await;

        // Query with local=true
        let resp = ctx.client.list_types(&QueryParams::new().local(true)).await;
        assert_eq!(resp.status, StatusCode::OK);

        // Mock should not have been called
        assert_eq!(
            mock.get_count(Method::GET, "/ngsi-ld/v1/types").await,
            0,
            "mock should not be called when local=true"
        );

        mock.stop();
    }));

    trials.push(trial(&format!("{PREFIX}::query_types_merges_remote_types"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:types_merge");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:types_merge");

        // Create a local entity so "Vehicle" type exists locally
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE_LD, &entity_id);
        let resp = ctx
            .client
            .request(
                Method::POST,
                "/entities?local=true",
                Some(&entity),
                crate::harness::client::RequestHeaders::new(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Create CSR pointing to mock
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns a type not present locally
        let mock_types = serde_json::json!({"id": "urn:ngsi-ld:EntityTypeList:mock", "type": "EntityTypeList", "typeList": ["Drone"]});
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/types", 200, &mock_types).await;

        // Query types without local
        let resp = ctx.client.list_types(&QueryParams::new()).await;
        assert_eq!(resp.status, StatusCode::OK);

        // Mock should have been called
        assert!(
            mock.get_count(Method::GET, "/ngsi-ld/v1/types").await > 0,
            "mock should be called when local is not set"
        );

        // Response should include the remote type "Drone"
        let body: serde_json::Value = resp.json_value();
        let type_list = body.get("typeList").and_then(|v| v.as_array());
        assert!(type_list.is_some(), "response should contain typeList");
        let types: Vec<&str> = type_list.unwrap().iter().filter_map(|v| v.as_str()).collect();
        assert!(types.contains(&"Drone"), "merged types should include Drone from remote, got: {types:?}");

        mock.stop();
    }));

    trials.push(trial(&format!("{PREFIX}::query_types_deduplicates"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:types_dedup");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:types_dedup");

        // Create local Vehicle entity
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE_LD, &entity_id);
        let resp = ctx
            .client
            .request(
                Method::POST,
                "/entities?local=true",
                Some(&entity),
                crate::harness::client::RequestHeaders::new(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Create CSR
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock also returns Vehicle (already local)
        let mock_types = serde_json::json!({"id": "urn:ngsi-ld:EntityTypeList:mock", "type": "EntityTypeList", "typeList": ["Vehicle"]});
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/types", 200, &mock_types).await;

        let resp = ctx.client.list_types(&QueryParams::new()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json_value();
        let type_list = body.get("typeList").and_then(|v| v.as_array()).expect("typeList should be present");
        let vehicle_count = type_list.iter().filter(|v| v.as_str() == Some("Vehicle")).count();
        assert_eq!(vehicle_count, 1, "Vehicle should appear once (deduplicated), got type_list: {type_list:?}");

        mock.stop();
    }));

    trials.push(trial(&format!("{PREFIX}::query_types_details_merges"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:types_details");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:types_details");

        // Create local entity
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE_LD, &entity_id);
        let resp = ctx
            .client
            .request(
                Method::POST,
                "/entities?local=true",
                Some(&entity),
                crate::harness::client::RequestHeaders::new(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Create CSR
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns detailed type info with a different type
        let mock_details = serde_json::json!([
            {"id": "Drone", "type": "EntityType", "typeName": "Drone", "attributeNames": ["altitude"]}
        ]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/types", 200, &mock_details).await;

        let resp = ctx
            .client
            .request(Method::GET, "/types?details=true", None, crate::harness::client::RequestHeaders::new())
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        // Check that remote types appear in detailed response
        let body: serde_json::Value = resp.json_value();
        let arr = body.as_array().expect("details=true should return array");
        let drone = arr.iter().find(|v| v.get("id").and_then(|id| id.as_str()) == Some("Drone"));
        assert!(drone.is_some(), "merged detailed types should include Drone, got: {arr:?}");

        mock.stop();
    }));

    trials.push(trial(&format!("{PREFIX}::query_types_warning_on_error"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:types_warn");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:types_warn");

        // Create CSR
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns 500
        mock.set_stub(Method::GET, "/ngsi-ld/v1/types", 500).await;

        let resp = ctx.client.list_types(&QueryParams::new()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let warnings = warning_header_values(&resp);
        assert!(!warnings.is_empty(), "expected NGSILD-Warning header on error, got none");
        assert!(
            warnings.iter().any(|w| w.starts_with("299")),
            "expected warning code 299 for error response, got: {warnings:?}"
        );

        mock.stop();
    }));

    trials.push(trial(&format!("{PREFIX}::query_types_warning_on_timeout"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:types_timeout");
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:types_timeout");
        let endpoint = mock.url("");

        // Create CSR pointing to mock, then stop mock to simulate unreachable
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &endpoint, None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        mock.stop();

        let resp = ctx.client.list_types(&QueryParams::new()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let warnings = warning_header_values(&resp);
        assert!(!warnings.is_empty(), "expected NGSILD-Warning header on timeout, got none");
        assert!(
            warnings.iter().any(|w| w.starts_with("199")),
            "expected warning code 199 for timeout, got: {warnings:?}"
        );
    }));

    trials.push(trial(&format!("{PREFIX}::query_type_info_merges_attributes"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:type_info_merge");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:type_info_merge");

        // Create local Vehicle entity
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE_LD, &entity_id);
        let resp = ctx
            .client
            .request(
                Method::POST,
                "/entities?local=true",
                Some(&entity),
                crate::harness::client::RequestHeaders::new(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Create CSR
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns type info with extra attributes
        let mock_type_info = serde_json::json!({
            "id": "Vehicle",
            "type": "EntityTypeInfo",
            "typeName": "Vehicle",
            "entityCount": 5,
            "attributeNames": ["extraAttr"]
        });
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/types/Vehicle", 200, &mock_type_info).await;

        let resp = ctx.client.get_type("Vehicle", &QueryParams::new()).await;
        assert_eq!(resp.status, StatusCode::OK);

        mock.stop();
    }));
}
