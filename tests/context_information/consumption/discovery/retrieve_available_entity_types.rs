use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial_clean_db,
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "consumption::discovery::entity_types";

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
    // 022_01_01: Retrieve entity types without JSON-LD context
    trials.push(trial_clean_db(&format!("{PREFIX}::022_01_01::without_context"), |ctx| async move {
        let building_id = ctx.entity_id("urn:ngsi-ld:Building:022_01_01");
        let vehicle_id = ctx.entity_id("urn:ngsi-ld:Vehicle:022_01_01");

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

        let resp = ctx.client.list_types(&QueryParams::new()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let expected = fixtures::load_expectation("types/expectations/entity-type-list-022-01-01.json");

        assert_eq!(body["type"], "EntityTypeList");
        assert_eq!(sorted_strings(&body["typeList"]), sorted_strings(&expected["typeList"]));
    }));

    // 022_01_02: Retrieve entity types with JSON-LD context
    trials.push(trial_clean_db(&format!("{PREFIX}::022_01_02::with_context"), |ctx| async move {
        let building_id = ctx.entity_id("urn:ngsi-ld:Building:022_01_02");
        let vehicle_id = ctx.entity_id("urn:ngsi-ld:Vehicle:022_01_02");

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
        let resp = ctx.client.request(Method::GET, "/types", None, headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let expected = fixtures::load_expectation("types/expectations/entity-type-list-022-01-02.json");

        assert_eq!(body["type"], "EntityTypeList");
        assert_eq!(sorted_strings(&body["typeList"]), sorted_strings(&expected["typeList"]));
    }));
}
