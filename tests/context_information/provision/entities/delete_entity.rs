use crate::harness::{client::QueryParams, fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "provision::entities::delete";

pub fn tests(trials: &mut Vec<Trial>) {
    // 002_01_01: Delete an existing entity
    trials.push(trial(&format!("{PREFIX}::002_01_01::success"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:002_01_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);

        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.delete_entity(&id).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT, "delete should return 204");

        // Verify entity is gone
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), Default::default()).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND, "entity should no longer exist");
    }));

    // 002_02_01: Delete entity without providing an ID or filters (DELETE on collection)
    trials.push(trial(&format!("{PREFIX}::002_02_01::missing_id"), |ctx| async move {
        let resp = ctx.client.request(Method::DELETE, "/entities", None, Default::default()).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "DELETE on /entities without filters should return 400");
    }));

    // 002_02_02: Delete entity with an invalid (non-URI) ID
    trials.push(trial(&format!("{PREFIX}::002_02_02::invalid_id"), |ctx| async move {
        let resp = ctx.client.delete_entity("thisisaninvaliduri").await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid URI should return 400");

        let body: serde_json::Value = resp.json();
        assert!(body["type"].as_str().is_some(), "should contain error type");
    }));

    // 002_03_01: Delete entity with an ID not known to the system
    trials.push(trial(&format!("{PREFIX}::002_03_01::not_found"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:002_03_01");
        let resp = ctx.client.delete_entity(&id).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND, "non-existent entity should return 404");

        let body: serde_json::Value = resp.json();
        assert!(
            body["type"].as_str().unwrap_or("").contains("ResourceNotFound"),
            "error type should be ResourceNotFound"
        );
    }));
}
