use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial_clean_db,
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "consumption::discovery::attributes";

fn sorted_strings(val: &serde_json::Value) -> Vec<String> {
    let mut v: Vec<String> = val.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect();
    v.sort();
    v
}

fn create_entity_json_headers() -> RequestHeaders {
    RequestHeaders::new()
        .content_type("application/json")
        .link_context(fixtures::compound_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // 025_01_01: Retrieve attributes without JSON-LD context
    trials.push(trial_clean_db(&format!("{PREFIX}::025_01_01::without_context"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:025_01_01");

        let entity = fixtures::load_fixture("entities/building-simple-attributes.json", &id);
        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), create_entity_json_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.list_attributes(&QueryParams::new()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let expected = fixtures::load_expectation("types/expectations/attribute-list-025-01-01.json");

        assert_eq!(body["type"], "AttributeList");
        assert_eq!(sorted_strings(&body["attributeList"]), sorted_strings(&expected["attributeList"]));
    }));

    // 025_01_02: Retrieve attributes with JSON-LD context
    trials.push(trial_clean_db(&format!("{PREFIX}::025_01_02::with_context"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:025_01_02");

        let entity = fixtures::load_fixture("entities/building-simple-attributes.json", &id);
        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), create_entity_json_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let headers = RequestHeaders::new().link_context(fixtures::compound_context_url());
        let resp = ctx.client.request(Method::GET, "/attributes", None, headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let expected = fixtures::load_expectation("types/expectations/attribute-list-025-01-02.json");

        assert_eq!(body["type"], "AttributeList");
        assert_eq!(sorted_strings(&body["attributeList"]), sorted_strings(&expected["attributeList"]));
    }));
}
