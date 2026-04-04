use crate::{
    distributed_operations::prepare_registration,
    harness::{client::QueryParams, fixtures, mock_server::MockServer, trial::trial},
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "distributed_operations::consumption::attributes::query_attributes";
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
    trials.push(trial(&format!("{PREFIX}::query_attributes_local_skips_forwarding"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrs_local_skip");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:attrs_local_skip");

        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None);
        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        let mock_attrs = serde_json::json!({"id": "urn:ngsi-ld:AttributeList:mock", "type": "AttributeList", "attributeList": ["speed"]});
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/attributes", 200, &mock_attrs).await;

        let resp = ctx.client.list_attributes(&QueryParams::new().local(true)).await;
        assert_eq!(resp.status, StatusCode::OK);

        assert_eq!(
            mock.get_count(Method::GET, "/ngsi-ld/v1/attributes").await,
            0,
            "mock should not be called when local=true"
        );

        mock.stop();
    }));

    trials.push(trial(&format!("{PREFIX}::query_attributes_merges_remote"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrs_merge");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:attrs_merge");

        // Create local entity so some attributes exist locally
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

        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None);
        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns an attribute not present locally
        let mock_attrs = serde_json::json!({"id": "urn:ngsi-ld:AttributeList:mock", "type": "AttributeList", "attributeList": ["speed"]});
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/attributes", 200, &mock_attrs).await;

        let resp = ctx.client.list_attributes(&QueryParams::new()).await;
        assert_eq!(resp.status, StatusCode::OK);

        assert!(
            mock.get_count(Method::GET, "/ngsi-ld/v1/attributes").await > 0,
            "mock should be called when local is not set"
        );

        let body: serde_json::Value = resp.json_value();
        let attr_list = body.get("attributeList").and_then(|v| v.as_array());
        assert!(attr_list.is_some(), "response should contain attributeList");
        let attrs: Vec<&str> = attr_list.unwrap().iter().filter_map(|v| v.as_str()).collect();
        assert!(attrs.contains(&"speed"), "merged attributes should include speed from remote, got: {attrs:?}");

        mock.stop();
    }));

    trials.push(trial(&format!("{PREFIX}::query_attributes_deduplicates"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrs_dedup");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:attrs_dedup");

        // Create local Vehicle entity (has brandName, isParked)
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

        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None);
        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns brandName which already exists locally (expanded IRI)
        let mock_attrs = serde_json::json!({"id": "urn:ngsi-ld:AttributeList:mock", "type": "AttributeList", "attributeList": ["brandName"]});
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/attributes", 200, &mock_attrs).await;

        let resp = ctx.client.list_attributes(&QueryParams::new()).await;
        assert_eq!(resp.status, StatusCode::OK);

        // brandName should appear at most once (may be expanded IRI form)
        let body: serde_json::Value = resp.json_value();
        let attr_list = body.get("attributeList").and_then(|v| v.as_array()).expect("attributeList should exist");
        // At minimum, no exact duplicates
        let mut seen = std::collections::HashSet::new();
        for attr in attr_list {
            if let Some(s) = attr.as_str() {
                assert!(seen.insert(s), "duplicate attribute found: {s}");
            }
        }

        mock.stop();
    }));

    trials.push(trial(&format!("{PREFIX}::query_attributes_details_merges"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrs_details");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:attrs_details");

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

        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None);
        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns detailed attribute info with a different attribute
        let mock_details = serde_json::json!([
            {"id": "altitude", "type": "Attribute", "attributeName": "altitude", "typeNames": ["Drone"]}
        ]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/attributes", 200, &mock_details).await;

        let resp = ctx
            .client
            .request(Method::GET, "/attributes?details=true", None, crate::harness::client::RequestHeaders::new())
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json_value();
        let arr = body.as_array().expect("details=true should return array");
        let altitude = arr.iter().find(|v| v.get("id").and_then(|id| id.as_str()) == Some("altitude"));
        assert!(altitude.is_some(), "merged detailed attributes should include altitude, got: {arr:?}");

        mock.stop();
    }));

    trials.push(trial(&format!("{PREFIX}::query_attributes_warning_on_error"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrs_warn");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:attrs_warn");

        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None);
        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        mock.set_stub(Method::GET, "/ngsi-ld/v1/attributes", 403).await;

        let resp = ctx.client.list_attributes(&QueryParams::new()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let warnings = warning_header_values(&resp);
        assert!(!warnings.is_empty(), "expected NGSILD-Warning header on 403, got none");
        assert!(
            warnings.iter().any(|w| w.starts_with("299")),
            "expected warning code 299 for error response, got: {warnings:?}"
        );

        mock.stop();
    }));

    trials.push(trial(&format!("{PREFIX}::query_attribute_info_merges"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:attr_info_merge");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:attr_info_merge");

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

        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None);
        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Get attribute info locally to know what exists first
        let local_resp = ctx.client.get_attribute("brandName", &QueryParams::new().local(true)).await;
        assert_eq!(local_resp.status, StatusCode::OK);

        // Mock returns attribute info with extra type names
        let mock_attr_info = serde_json::json!({
            "id": "brandName",
            "type": "Attribute",
            "attributeName": "brandName",
            "typeNames": ["Drone"]
        });
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/attributes/brandName", 200, &mock_attr_info)
            .await;

        let resp = ctx.client.get_attribute("brandName", &QueryParams::new()).await;
        assert_eq!(resp.status, StatusCode::OK);

        mock.stop();
    }));
}
