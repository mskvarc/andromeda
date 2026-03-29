use crate::harness::{
    TestCtx,
    client::ContextQueryParams,
    context_server::ContextServer,
    fixtures,
    trial::{trial, trial_clean_db},
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use std::collections::HashMap;

const PREFIX: &str = "jsonld_context::consumption::serve";

/// Helper: create a hosted context and return its ID (already tracked for cleanup).
async fn create_hosted_context(ctx: &TestCtx) -> String {
    let body = fixtures::load_fragment("jsonld_context/@context-minimal-valid.json");
    let resp = ctx.client.create_context(&body).await;
    assert_eq!(resp.status, StatusCode::CREATED);
    let id = resp.location().unwrap().rsplit('/').next().unwrap().to_string();
    ctx.track_context(&id);
    id
}

/// Helper: start a local context server, create entity to cache context, return server + cached ID + url.
async fn setup_cached_context(ctx: &TestCtx) -> (ContextServer, String, String) {
    let ctx_content = r#"{"@context":{"X1":"urn:ngsi-ld:attributes:X1"}}"#;
    let server = ContextServer::start(HashMap::from([("/context.jsonld".to_string(), ctx_content.to_string())])).await;
    let ctx_url = server.url("/context.jsonld");

    let entity_id = ctx.entity_id("urn:ngsi-ld:Testing");
    let entity = serde_json::json!({
        "id": &entity_id,
        "type": "Testing",
        "X1": {
            "type": "Property",
            "value": "test"
        },
        "@context": [&ctx_url]
    });
    let resp = ctx.client.create_entity(&entity).await;
    assert_eq!(resp.status, StatusCode::CREATED);

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Find cached context ID
    let params = ContextQueryParams::new().kind("Cached").details(true);
    let resp = ctx.client.list_contexts_with_params(&params).await;
    let cached: Vec<serde_json::Value> = resp.json();
    let cached_ctx = cached
        .iter()
        .find(|c| c.get("URL").and_then(|u| u.as_str()).map_or(false, |u| u == ctx_url))
        .expect("context should be cached");
    let cached_id = cached_ctx["localId"].as_str().unwrap().to_string();

    (server, cached_id, ctx_url)
}

pub fn tests(trials: &mut Vec<Trial>) {
    // 053_01_01: Serve @context without details parameter
    trials.push(trial_clean_db(&format!("{PREFIX}::053_01_01::serve_context_no_details"), |ctx| async move {
        let ctx_id = create_hosted_context(&ctx).await;

        let resp = ctx.client.get_context(&ctx_id, &ContextQueryParams::new()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body = resp.json_value();
        let c = body.get("@context").expect("response should contain @context");
        assert_eq!(c["A1"], "urn:ngsi-ld:attributes:A1");
        assert_eq!(c["A2"], "urn:ngsi-ld:attributes:A2");
        assert_eq!(c["A3"], "urn:ngsi-ld:attributes:A3");
    }));

    // 053_01_02: Serve @context with details=false
    trials.push(trial_clean_db(&format!("{PREFIX}::053_01_02::serve_context_details_false"), |ctx| async move {
        let ctx_id = create_hosted_context(&ctx).await;

        let params = ContextQueryParams::new().details(false);
        let resp = ctx.client.get_context(&ctx_id, &params).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body = resp.json_value();
        let c = body.get("@context").expect("response should contain @context");
        assert_eq!(c["A1"], "urn:ngsi-ld:attributes:A1");
    }));

    // 053_02_01: Serve @context with details=true
    trials.push(trial_clean_db(&format!("{PREFIX}::053_02_01::serve_context_details_true"), |ctx| async move {
        let ctx_id = create_hosted_context(&ctx).await;

        let params = ContextQueryParams::new().details(true);
        let resp = ctx.client.get_context(&ctx_id, &params).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body = resp.json_value();
        assert!(body.get("URL").is_some(), "detailed response should have URL");
        assert_eq!(body["kind"], "Hosted");
        assert!(body.get("createdAt").is_some(), "detailed response should have createdAt");
    }));

    // 053_03_01: Serve @context with unknown ID → 404
    trials.push(trial(&format!("{PREFIX}::053_03_01::serve_unknown_context"), |ctx| async move {
        let resp = ctx.client.get_context("urn:ngsi-ld:unknown:ctx:12345", &ContextQueryParams::new()).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
        let body = resp.json_value();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound");
    }));

    // 053_04_01: numberOfHits increases after using hosted context
    trials.push(trial_clean_db(&format!("{PREFIX}::053_04_01::serve_hosted_number_of_hits"), |ctx| async move {
        let ctx_id = create_hosted_context(&ctx).await;

        // Check initial numberOfHits
        let params = ContextQueryParams::new().details(true);
        let resp = ctx.client.get_context(&ctx_id, &params).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body = resp.json_value();
        let initial_hits = body["numberOfHits"].as_u64().unwrap_or(0);

        // Get the full context URL from the detailed response
        let ctx_url = body["URL"].as_str().expect("should have URL").to_string();

        // Create an entity using this context to increment hits
        let entity_id = ctx.entity_id("urn:ngsi-ld:Testing");
        let entity = serde_json::json!({
            "id": &entity_id,
            "type": "Testing",
            "A1": {
                "type": "Property",
                "value": "value of A1"
            },
            "@context": [&ctx_url]
        });
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Check numberOfHits increased
        let resp = ctx.client.get_context(&ctx_id, &params).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body = resp.json_value();
        let new_hits = body["numberOfHits"].as_u64().unwrap_or(0);
        assert!(new_hits > initial_hits, "numberOfHits should have increased: {new_hits} > {initial_hits}");
    }));

    // 053_07_01: Serve ImplicitlyCreated @context with details=true
    trials.push(trial_clean_db(
        &format!("{PREFIX}::053_07_01::serve_implicitly_created_details"),
        |ctx| async move {
            let sub = serde_json::json!({
                "type": "Subscription",
                "entities": [{"type": "T1"}],
                "notification": {
                    "endpoint": {
                        "uri": "http://a.b.c/123"
                    }
                },
                "@context": [
                    "https://fiware.github.io/NGSI-LD_TestSuite/ldContext/testContext.jsonld",
                    "http://smartdatamodels.org/context.jsonld"
                ]
            });
            let resp = ctx.client.create_subscription(&sub).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            let sub_id = resp.location().unwrap().rsplit('/').next().unwrap().to_string();
            ctx.track_subscription(&sub_id);

            // Find the implicit context
            let list_params = ContextQueryParams::new().kind("ImplicitlyCreated");
            let resp = ctx.client.list_contexts_with_params(&list_params).await;
            let contexts: Vec<serde_json::Value> = resp.json();
            let ctx_uri = contexts.first().expect("should have implicit context").as_str().unwrap().to_string();
            let ctx_id = ctx_uri.rsplit('/').next().unwrap().to_string();

            // Serve with details
            let params = ContextQueryParams::new().details(true);
            let resp = ctx.client.get_context(&ctx_id, &params).await;
            assert_eq!(resp.status, StatusCode::OK);

            let body = resp.json_value();
            assert_eq!(body["kind"], "ImplicitlyCreated");
            assert!(body.get("URL").is_some());
            assert!(body.get("createdAt").is_some());
        },
    ));

    // 053_08_01: numberOfHits increases after using ImplicitlyCreated context
    trials.push(trial_clean_db(
        &format!("{PREFIX}::053_08_01::serve_implicitly_created_number_of_hits"),
        |ctx| async move {
            let sub = serde_json::json!({
                "type": "Subscription",
                "entities": [{"type": "T1"}],
                "notification": {
                    "endpoint": {
                        "uri": "http://a.b.c/123"
                    }
                },
                "@context": [
                    "https://fiware.github.io/NGSI-LD_TestSuite/ldContext/testContext.jsonld",
                    "http://smartdatamodels.org/context.jsonld"
                ]
            });
            let resp = ctx.client.create_subscription(&sub).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            let sub_id = resp.location().unwrap().rsplit('/').next().unwrap().to_string();
            ctx.track_subscription(&sub_id);

            // Find the implicit context
            let list_params = ContextQueryParams::new().kind("ImplicitlyCreated");
            let resp = ctx.client.list_contexts_with_params(&list_params).await;
            let contexts: Vec<serde_json::Value> = resp.json();
            let ctx_uri = contexts.first().expect("should have implicit context").as_str().unwrap().to_string();
            let ctx_id = ctx_uri.rsplit('/').next().unwrap().to_string();

            // Check initial hits
            let detail_params = ContextQueryParams::new().details(true);
            let resp = ctx.client.get_context(&ctx_id, &detail_params).await;
            let body = resp.json_value();
            let initial_hits = body["numberOfHits"].as_u64().unwrap_or(0);
            let ctx_url = body["URL"].as_str().expect("should have URL").to_string();

            // Create entity using this context
            let entity_id = ctx.entity_id("urn:ngsi-ld:Testing");
            let entity = serde_json::json!({
                "id": &entity_id,
                "type": "Testing",
                "A1": {
                    "type": "Property",
                    "value": "value"
                },
                "@context": [&ctx_url]
            });
            let resp = ctx.client.create_entity(&entity).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            // Check hits increased
            let resp = ctx.client.get_context(&ctx_id, &detail_params).await;
            let body = resp.json_value();
            let new_hits = body["numberOfHits"].as_u64().unwrap_or(0);
            assert!(new_hits > initial_hits, "numberOfHits should increase: {new_hits} > {initial_hits}");
        },
    ));

    // 053_05_01: Cached context still available after context server goes down
    trials.push(trial_clean_db(
        &format!("{PREFIX}::053_05_01::serve_cached_context_after_server_down"),
        |ctx| async move {
            let (mut server, cached_id, _ctx_url) = setup_cached_context(&ctx).await;

            // Stop the context server
            server.stop();
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;

            // Attempt delete with reload (should fail)
            let params = ContextQueryParams::new().reload(true);
            let resp = ctx.client.delete_context_with_params(&cached_id, &params).await;
            assert_eq!(resp.status, StatusCode::SERVICE_UNAVAILABLE);

            // But GET should still work from cache
            let params = ContextQueryParams::new().details(true);
            let resp = ctx.client.get_context(&cached_id, &params).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body = resp.json_value();
            assert_eq!(body["kind"], "Cached");
            assert!(body.get("URL").is_some());
            assert!(body.get("createdAt").is_some());
        },
    ));

    // 053_06_01: numberOfHits increases for cached context
    trials.push(trial_clean_db(&format!("{PREFIX}::053_06_01::serve_cached_number_of_hits"), |ctx| async move {
        let ctx_content = r#"{"@context":{"X1":"urn:ngsi-ld:attributes:X1"}}"#;
        let server = ContextServer::start(HashMap::from([("/context.jsonld".to_string(), ctx_content.to_string())])).await;
        let ctx_url = server.url("/context.jsonld");

        // Create first entity
        let entity_id1 = ctx.entity_id("urn:ngsi-ld:Testing");
        let entity = serde_json::json!({
            "id": &entity_id1,
            "type": "Testing",
            "X1": {
                "type": "Property",
                "value": "test1"
            },
            "@context": [&ctx_url]
        });
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // Find cached context and check initial hits
        let params = ContextQueryParams::new().kind("Cached").details(true);
        let resp = ctx.client.list_contexts_with_params(&params).await;
        let cached: Vec<serde_json::Value> = resp.json();
        let cached_ctx = cached
            .iter()
            .find(|c| c.get("URL").and_then(|u| u.as_str()).map_or(false, |u| u == ctx_url))
            .expect("context should be cached");
        let cached_id = cached_ctx["localId"].as_str().unwrap().to_string();
        let hits_after_first = cached_ctx["numberOfHits"].as_u64().unwrap_or(0);

        // Create second entity using same context
        let entity_id2 = ctx.entity_id("urn:ngsi-ld:Testing");
        let entity = serde_json::json!({
            "id": &entity_id2,
            "type": "Testing",
            "X1": {
                "type": "Property",
                "value": "test2"
            },
            "@context": [&ctx_url]
        });
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Check hits increased
        let detail_params = ContextQueryParams::new().details(true);
        let resp = ctx.client.get_context(&cached_id, &detail_params).await;
        let body = resp.json_value();
        let hits_after_second = body["numberOfHits"].as_u64().unwrap_or(0);
        assert!(
            hits_after_second > hits_after_first,
            "numberOfHits should increase: {hits_after_second} > {hits_after_first}"
        );

        let _ = server;
    }));
}
