use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial_clean_db,
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "consumption::discovery::entity_type_info";

fn create_entity_json_headers() -> RequestHeaders {
    RequestHeaders::new()
        .content_type("application/json")
        .link_context(fixtures::compound_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // 024_01_01: Retrieve detailed representation of unknown entity type without context
    trials.push(trial_clean_db(&format!("{PREFIX}::024_01_01::unknown_type_404"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:024_01_01a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:024_01_01b");

        let e1 = fixtures::load_fixture("entities/building-simple-attributes.json", &id1);
        let e2 = fixtures::load_fixture("entities/building-simple-attributes.json", &id2);

        let resp = ctx.client.request(Method::POST, "/entities", Some(&e1), create_entity_json_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let resp = ctx.client.request(Method::POST, "/entities", Some(&e2), create_entity_json_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.get_type("Building", &QueryParams::new()).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound");
        assert!(body["title"].as_str().is_some_and(|t| !t.is_empty()), "title should be a non-empty string");
    }));

    // 024_02_01: Retrieve entity type info with compacted type name
    trials.push(trial_clean_db(&format!("{PREFIX}::024_02_01::compacted_type"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:024_02_01a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:024_02_01b");

        let e1 = fixtures::load_fixture("entities/building-simple-attributes.json", &id1);
        let e2 = fixtures::load_fixture("entities/building-simple-attributes.json", &id2);

        let resp = ctx.client.request(Method::POST, "/entities", Some(&e1), create_entity_json_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let resp = ctx.client.request(Method::POST, "/entities", Some(&e2), create_entity_json_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let headers = RequestHeaders::new().link_context(fixtures::compound_context_url());
        let resp = ctx.client.request(Method::GET, "/types/Building", None, headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let expected = fixtures::load_expectation("types/expectations/entity-type-info-024-01.json");

        assert_eq!(body["type"], expected["type"]);
        assert_eq!(body["typeName"], expected["typeName"]);
        assert_eq!(body["entityCount"], expected["entityCount"]);

        let mut body_details: Vec<serde_json::Value> = body["attributeDetails"].as_array().unwrap().clone();
        body_details.sort_by(|a, b| a["attributeName"].as_str().unwrap().cmp(b["attributeName"].as_str().unwrap()));
        let mut expected_details: Vec<serde_json::Value> = expected["attributeDetails"].as_array().unwrap().clone();
        expected_details.sort_by(|a, b| a["attributeName"].as_str().unwrap().cmp(b["attributeName"].as_str().unwrap()));
        assert_eq!(body_details, expected_details);
    }));

    // 024_02_02: Retrieve entity type info with expanded (URL-encoded) type name
    trials.push(trial_clean_db(&format!("{PREFIX}::024_02_02::expanded_type"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:024_02_02a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:024_02_02b");

        let e1 = fixtures::load_fixture("entities/building-simple-attributes.json", &id1);
        let e2 = fixtures::load_fixture("entities/building-simple-attributes.json", &id2);

        let resp = ctx.client.request(Method::POST, "/entities", Some(&e1), create_entity_json_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let resp = ctx.client.request(Method::POST, "/entities", Some(&e2), create_entity_json_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let expanded_type = "https%3A%2F%2Fngsi-ld-test-suite%2Fcontext%23Building";
        let headers = RequestHeaders::new().link_context(fixtures::compound_context_url());
        let resp = ctx.client.request(Method::GET, &format!("/types/{expanded_type}"), None, headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let expected = fixtures::load_expectation("types/expectations/entity-type-info-024-01.json");

        assert_eq!(body["type"], expected["type"]);
        assert_eq!(body["typeName"], expected["typeName"]);
        assert_eq!(body["entityCount"], expected["entityCount"]);

        let mut body_details: Vec<serde_json::Value> = body["attributeDetails"].as_array().unwrap().clone();
        body_details.sort_by(|a, b| a["attributeName"].as_str().unwrap().cmp(b["attributeName"].as_str().unwrap()));
        let mut expected_details: Vec<serde_json::Value> = expected["attributeDetails"].as_array().unwrap().clone();
        expected_details.sort_by(|a, b| a["attributeName"].as_str().unwrap().cmp(b["attributeName"].as_str().unwrap()));
        assert_eq!(body_details, expected_details);
    }));
}
