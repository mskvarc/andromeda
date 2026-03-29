use crate::harness::{client::RequestHeaders, fixtures, trial::trial_clean_db};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "consumption::discovery::attribute_details";

/// Sort an array of Attribute objects by attributeName for stable comparison.
fn sort_attributes(val: &serde_json::Value) -> Vec<serde_json::Value> {
    let mut arr: Vec<serde_json::Value> = val.as_array().unwrap().clone();
    arr.sort_by(|a, b| a["attributeName"].as_str().unwrap().cmp(b["attributeName"].as_str().unwrap()));
    arr
}

fn create_entity_json_headers() -> RequestHeaders {
    RequestHeaders::new()
        .content_type("application/json")
        .link_context(fixtures::compound_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // 026_01_01: Retrieve attribute details without JSON-LD context
    trials.push(trial_clean_db(&format!("{PREFIX}::026_01_01::without_context"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:026_01_01");

        let entity = fixtures::load_fixture("entities/building-simple-attributes.json", &id);
        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), create_entity_json_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.request(Method::GET, "/attributes?details=true", None, Default::default()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let expected = fixtures::load_expectation("types/expectations/attribute-026-01-01.json");

        assert_eq!(sort_attributes(&body), sort_attributes(&expected));
    }));

    // 026_01_02: Retrieve attribute details with JSON-LD context
    trials.push(trial_clean_db(&format!("{PREFIX}::026_01_02::with_context"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:026_01_02");

        let entity = fixtures::load_fixture("entities/building-simple-attributes.json", &id);
        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), create_entity_json_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let headers = RequestHeaders::new().link_context(fixtures::compound_context_url());
        let resp = ctx.client.request(Method::GET, "/attributes?details=true", None, headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let expected = fixtures::load_expectation("types/expectations/attribute-026-01-02.json");

        assert_eq!(sort_attributes(&body), sort_attributes(&expected));
    }));
}
