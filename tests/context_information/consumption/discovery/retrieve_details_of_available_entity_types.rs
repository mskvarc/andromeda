use crate::harness::{client::RequestHeaders, fixtures, trial::trial_clean_db};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "consumption::discovery::entity_type_details";

fn sorted_strings(val: &serde_json::Value) -> Vec<String> {
    let mut v: Vec<String> = val.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect();
    v.sort();
    v
}

/// Sort an array of EntityType objects by typeName for stable comparison.
fn sort_entity_types(val: &serde_json::Value) -> Vec<serde_json::Value> {
    let mut arr: Vec<serde_json::Value> = val.as_array().unwrap().clone();
    arr.sort_by(|a, b| a["typeName"].as_str().unwrap().cmp(b["typeName"].as_str().unwrap()));
    for item in &mut arr {
        if let Some(names) = item.get_mut("attributeNames") {
            let mut sorted = sorted_strings(names);
            sorted.sort();
            *names = serde_json::Value::Array(sorted.into_iter().map(serde_json::Value::String).collect());
        }
    }
    arr
}

fn create_entity_json_headers() -> RequestHeaders {
    RequestHeaders::new()
        .content_type("application/json")
        .link_context(fixtures::compound_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // 023_01_01: Retrieve entity type details without JSON-LD context
    trials.push(trial_clean_db(&format!("{PREFIX}::023_01_01::without_context"), |ctx| async move {
        let building_id = ctx.entity_id("urn:ngsi-ld:Building:023_01_01");
        let vehicle_id = ctx.entity_id("urn:ngsi-ld:Vehicle:023_01_01");

        let building = fixtures::load_fixture("entities/building-simple-attributes.json", &building_id);
        let vehicle = fixtures::load_fixture("entities/vehicle-simple-attributes.json", &vehicle_id);

        let resp = ctx
            .client
            .request(Method::POST, "/entities", Some(&building), create_entity_json_headers())
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let resp = ctx
            .client
            .request(Method::POST, "/entities", Some(&vehicle), create_entity_json_headers())
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.request(Method::GET, "/types?details=true", None, Default::default()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let expected = fixtures::load_expectation("types/expectations/entity-type-023-01-01.json");

        assert_eq!(sort_entity_types(&body), sort_entity_types(&expected));
    }));

    // 023_01_02: Retrieve entity type details with JSON-LD context
    trials.push(trial_clean_db(&format!("{PREFIX}::023_01_02::with_context"), |ctx| async move {
        let building_id = ctx.entity_id("urn:ngsi-ld:Building:023_01_02");
        let vehicle_id = ctx.entity_id("urn:ngsi-ld:Vehicle:023_01_02");

        let building = fixtures::load_fixture("entities/building-simple-attributes.json", &building_id);
        let vehicle = fixtures::load_fixture("entities/vehicle-simple-attributes.json", &vehicle_id);

        let resp = ctx
            .client
            .request(Method::POST, "/entities", Some(&building), create_entity_json_headers())
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let resp = ctx
            .client
            .request(Method::POST, "/entities", Some(&vehicle), create_entity_json_headers())
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let headers = RequestHeaders::new().link_context(fixtures::compound_context_url());
        let resp = ctx.client.request(Method::GET, "/types?details=true", None, headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let expected = fixtures::load_expectation("types/expectations/entity-type-023-01-02.json");

        assert_eq!(sort_entity_types(&body), sort_entity_types(&expected));
    }));
}
