use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial_clean_db,
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "consumption::discovery::attribute_info";

fn create_entity_json_headers() -> RequestHeaders {
    RequestHeaders::new()
        .content_type("application/json")
        .link_context(fixtures::compound_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // 027_01_01: Retrieve detailed representation of unknown attribute without context
    trials.push(trial_clean_db(&format!("{PREFIX}::027_01_01::unknown_attribute_404"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:027_01_01");

        let entity = fixtures::load_fixture("entities/building-simple-attributes.json", &id);
        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), create_entity_json_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.get_attribute("airQualityLevel", &QueryParams::new()).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound");
        assert!(body["title"].as_str().is_some_and(|t| !t.is_empty()), "title should be a non-empty string");
    }));

    // 027_02_01: Retrieve attribute info with compacted attribute name
    trials.push(trial_clean_db(&format!("{PREFIX}::027_02_01::compacted_name"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:027_02_01");

        let entity = fixtures::load_fixture("entities/building-simple-attributes.json", &id);
        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), create_entity_json_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let headers = RequestHeaders::new().link_context(fixtures::compound_context_url());
        let resp = ctx.client.request(Method::GET, "/attributes/airQualityLevel", None, headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let expected = fixtures::load_expectation("types/expectations/attribute-027-01.json");

        assert_eq!(body["type"], expected["type"]);
        assert_eq!(body["attributeName"], expected["attributeName"]);
        assert_eq!(body["attributeCount"], expected["attributeCount"]);
        assert_eq!(body["attributeTypes"], expected["attributeTypes"]);
        assert_eq!(body["typeNames"], expected["typeNames"]);
    }));

    // 027_02_02: Retrieve attribute info with expanded (URL-encoded) attribute name
    trials.push(trial_clean_db(&format!("{PREFIX}::027_02_02::expanded_name"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:027_02_02");

        let entity = fixtures::load_fixture("entities/building-simple-attributes.json", &id);
        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), create_entity_json_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let expanded_attr = "https%3A%2F%2Fngsi-ld-test-suite%2Fcontext%23airQualityLevel";
        let headers = RequestHeaders::new().link_context(fixtures::compound_context_url());
        let resp = ctx.client.request(Method::GET, &format!("/attributes/{expanded_attr}"), None, headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let expected = fixtures::load_expectation("types/expectations/attribute-027-01.json");

        assert_eq!(body["type"], expected["type"]);
        assert_eq!(body["attributeName"], expected["attributeName"]);
        assert_eq!(body["attributeCount"], expected["attributeCount"]);
        assert_eq!(body["attributeTypes"], expected["attributeTypes"]);
        assert_eq!(body["typeNames"], expected["typeNames"]);
    }));
}
