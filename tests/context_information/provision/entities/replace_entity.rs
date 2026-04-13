#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::harness::{
    client::{NgsiLdOption, QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "provision::entities::replace";

pub fn tests(trials: &mut Vec<Trial>) {
    // 054_01_01: Replace an existing entity, createdAt should be preserved
    trials.push(trial(&format!("{PREFIX}::054_01_01::success"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:054_01_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.json", &id);

        let headers = RequestHeaders::new()
            .content_type("application/json")
            .link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Get createdAt before replacement
        let params = QueryParams::new().options([NgsiLdOption::SysAttrs]);
        let resp = ctx.client.get_entity(&id, &params, Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let before: serde_json::Value = resp.json();
        let created_at = before["createdAt"].as_str().unwrap().to_string();

        // Replace entity
        let replacement = fixtures::load_fixture("entities/building-locatedAt-and-name.json", &id);
        let headers = RequestHeaders::new()
            .content_type("application/json")
            .link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.request(Method::PUT, &format!("/entities/{id}"), Some(&replacement), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT, "replace should return 204");

        // Verify replacement
        let resp = ctx.client.get_entity(&id, &params, Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let after: serde_json::Value = resp.json();

        pretty_assertions::assert_eq!(after["name"]["value"], "Pisa Tower", "name should be updated");
        pretty_assertions::assert_eq!(after["locatedAt"]["object"], "urn:ngsi-ld:City:Pisa", "new relationship should be present");
        assert!(after.get("category").is_none(), "old attribute should be gone");
        pretty_assertions::assert_eq!(after["createdAt"].as_str().unwrap(), created_at, "createdAt should be preserved");
    }));

    // 054_02_01: Replace entity with invalid ID
    trials.push(trial(&format!("{PREFIX}::054_02_01::invalid_id"), |ctx| async move {
        let replacement = serde_json::json!({
            "id": "invalidUri",
            "type": "Building",
            "name": {
                "type": "Property",
                "value": "Pisa Tower"
            },
            "@context": fixtures::test_suite_context_url()
        });

        let resp = ctx.client.replace_entity("invalidUri", &replacement).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST, "invalid ID should return 400");
    }));

    // 054_02_02: Replace entity without providing an ID (PUT on collection)
    trials.push(trial(&format!("{PREFIX}::054_02_02::missing_id"), |ctx| async move {
        let replacement = serde_json::json!({
            "type": "Building",
            "name": {
                "type": "Property",
                "value": "Pisa Tower"
            },
            "@context": fixtures::test_suite_context_url()
        });

        let resp = ctx.client.request(Method::PUT, "/entities", Some(&replacement), Default::default()).await;
        // PUT on /entities (collection) without an ID
        assert!(
            resp.status == StatusCode::BAD_REQUEST || resp.status == StatusCode::METHOD_NOT_ALLOWED,
            "missing ID should return 400 or 405, got {}",
            resp.status
        );
    }));

    // 054_02_03: Replace entity with non-existent ID
    trials.push(trial(&format!("{PREFIX}::054_02_03::not_found"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:054_02_03");
        let replacement = serde_json::json!({
            "id": &id,
            "type": "Building",
            "name": {
                "type": "Property",
                "value": "Pisa Tower"
            },
            "@context": fixtures::test_suite_context_url()
        });

        let resp = ctx.client.replace_entity(&id, &replacement).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND, "non-existent entity should return 404");
    }));

    // 054_03_01: Replace entity with scopes
    trials.push(trial(&format!("{PREFIX}::054_03_01::with_scopes"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:054_03_01");
        let entity = fixtures::load_fixture("entities/building-minimal-with-one-scope.json", &id);

        let headers = RequestHeaders::new()
            .content_type("application/json")
            .link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.request(Method::POST, "/entities", Some(&entity), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Replace with multiple scopes
        let replacement = fixtures::load_fixture("entities/building-minimal-with-many-scopes.json", &id);
        let headers = RequestHeaders::new()
            .content_type("application/json")
            .link_context(fixtures::test_suite_context_url());
        let resp = ctx.client.request(Method::PUT, &format!("/entities/{id}"), Some(&replacement), headers).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT, "replace should return 204");

        // Verify scopes replaced
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        let scopes = body["scope"].as_array().expect("scope should be an array");
        pretty_assertions::assert_eq!(scopes.len(), 2);
    }));
}
