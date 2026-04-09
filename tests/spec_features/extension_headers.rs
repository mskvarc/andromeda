use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use reqwest::StatusCode;

const PREFIX: &str = "spec_features::extension_headers";

fn ctx_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    trials.push(trial(&format!("{PREFIX}::response_format_header_fallback"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:ext-format-fallback");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "name": {"type": "Property", "value": "HQ"},
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let headers = ctx_headers().response_format("keyValues");
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body = resp.json_value();
        assert_eq!(body["name"], "HQ");
    }));

    trials.push(trial(&format!("{PREFIX}::response_format_query_precedence"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:ext-format-precedence");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "name": {"type": "Property", "value": "HQ"},
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().format("normalized");
        let headers = ctx_headers().response_format("keyValues");
        let resp = ctx.client.get_entity(&id, &params, headers).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body = resp.json_value();
        assert_eq!(body["name"]["type"], "Property");
    }));

    trials.push(trial(&format!("{PREFIX}::dataset_header_fallback"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:ext-dataset-fallback");
        let entity = fixtures::load_fixture("entities/vehicle-two-datasetid-attributes.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let dataset = "urn:ngsi-ld:Property:gpsBxyz123-speed";
        let header_resp = ctx.client.get_entity(&id, &QueryParams::new(), ctx_headers().dataset_id(dataset)).await;
        assert_eq!(header_resp.status, StatusCode::OK);

        let query_resp = ctx.client.get_entity(&id, &QueryParams::new().dataset_id(dataset), ctx_headers()).await;
        assert_eq!(query_resp.status, StatusCode::OK);

        assert_eq!(
            header_resp.json_value(),
            query_resp.json_value(),
            "NGSILD-Dataset-Id fallback must match datasetId query behavior"
        );
    }));

    trials.push(trial(&format!("{PREFIX}::dataset_query_precedence"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:ext-dataset-precedence");
        let entity = fixtures::load_fixture("entities/vehicle-two-datasetid-attributes.jsonld", &id);
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let query_dataset = "urn:ngsi-ld:Relationship:parked12345";
        let header_dataset = "urn:ngsi-ld:Property:gpsBxyz123-speed";

        let query_only = ctx.client.get_entity(&id, &QueryParams::new().dataset_id(query_dataset), ctx_headers()).await;
        assert_eq!(query_only.status, StatusCode::OK);

        let query_and_header = ctx
            .client
            .get_entity(&id, &QueryParams::new().dataset_id(query_dataset), ctx_headers().dataset_id(header_dataset))
            .await;
        assert_eq!(query_and_header.status, StatusCode::OK);

        assert_eq!(
            query_and_header.json_value(),
            query_only.json_value(),
            "datasetId query parameter must take precedence over NGSILD-Dataset-Id"
        );
    }));

    trials.push(trial(&format!("{PREFIX}::join_headers_fallback"), |ctx| async move {
        let linked_id = "urn:ngsi-ld:Entity:ext-join-linked";
        ctx.track_entity(linked_id);
        let linked = serde_json::json!({
            "id": linked_id,
            "type": "Building",
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&linked).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let id = ctx.entity_id("urn:ngsi-ld:Entity:ext-join-root");
        let root = serde_json::json!({
            "id": id,
            "type": "Building",
            "linkedTo": {"type": "Relationship", "object": linked_id},
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&root).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().id(id.clone()).type_("Building");
        let headers = ctx_headers().join("flat").join_level(1);
        let resp = ctx.client.query_entities(&params, headers).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body = resp.json_value();
        let arr = body.as_array().expect("flat join query must return array");
        let ids: Vec<&str> = arr.iter().filter_map(|item| item["id"].as_str()).collect();
        assert!(ids.contains(&id.as_str()));
        assert!(ids.contains(&linked_id));
    }));

    trials.push(trial(&format!("{PREFIX}::scope_header_fallback"), |ctx| async move {
        let id_a = ctx.entity_id("urn:ngsi-ld:Building:ext-scope-a");
        let id_b = ctx.entity_id("urn:ngsi-ld:Building:ext-scope-b");
        let entity_a = serde_json::json!({
            "id": id_a,
            "type": "Building",
            "scope": "/Company/AreaA",
            "@context": [fixtures::test_suite_context_url()]
        });
        let entity_b = serde_json::json!({
            "id": id_b,
            "type": "Building",
            "scope": "/Company/AreaB",
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&entity_a).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let resp = ctx.client.create_entity(&entity_b).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Building");
        let headers = ctx_headers().scope("/Company/AreaA");
        let resp = ctx.client.query_entities(&params, headers).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body = resp.json_value();
        let arr = body.as_array().expect("query result array");
        let ids: Vec<&str> = arr.iter().filter_map(|item| item["id"].as_str()).collect();
        assert!(ids.contains(&id_a.as_str()));
        assert!(!ids.contains(&id_b.as_str()));
    }));
}
