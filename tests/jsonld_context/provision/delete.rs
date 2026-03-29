use crate::harness::{
    TestCtx,
    client::ContextQueryParams,
    context_server::ContextServer,
    fixtures,
    trial::{trial, trial_clean_db},
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use std::collections::HashMap;

const PREFIX: &str = "jsonld_context::provision::delete";

/// Helper: create a hosted context and return its ID (already tracked for cleanup).
async fn create_hosted_context(ctx: &TestCtx) -> String {
    let body = fixtures::load_fragment("jsonld_context/@context-minimal-valid.json");
    let resp = ctx.client.create_context(&body).await;
    assert_eq!(resp.status, StatusCode::CREATED);
    let id = resp.location().unwrap().rsplit('/').next().unwrap().to_string();
    ctx.track_context(&id);
    id
}

/// Helper: create a subscription that implicitly creates a context, return the implicit context ID.
async fn create_implicit_context(ctx: &TestCtx) -> (String, String) {
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

    // Find the implicitly created context
    let params = ContextQueryParams::new().kind("ImplicitlyCreated");
    let resp = ctx.client.list_contexts_with_params(&params).await;
    let contexts: Vec<serde_json::Value> = resp.json();
    let ctx_id = contexts
        .first()
        .expect("should have implicit context")
        .as_str()
        .expect("context should be a string URI")
        .rsplit('/')
        .next()
        .unwrap()
        .to_string();

    (sub_id, ctx_id)
}

/// Helper: start a local context server, create an entity that references it (causing caching),
/// then delete the entity. Returns (server, cached_context_url).
async fn setup_cached_context(ctx: &TestCtx) -> (ContextServer, String) {
    let ctx_content = r#"{"@context":{"X1":"urn:ngsi-ld:attributes:X1"}}"#;
    let server = ContextServer::start(HashMap::from([("/context.jsonld".to_string(), ctx_content.to_string())])).await;
    let ctx_url = server.url("/context.jsonld");

    // Create entity using the external context to trigger caching
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

    // Wait briefly for caching
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Clean up entity but keep context cached — entity is already tracked via ctx.entity_id

    (server, ctx_url)
}

pub fn tests(trials: &mut Vec<Trial>) {
    // 051_01_01: Delete hosted @context without reload param
    trials.push(trial_clean_db(&format!("{PREFIX}::051_01_01::delete_hosted_context"), |ctx| async move {
        let ctx_id = create_hosted_context(&ctx).await;

        let resp = ctx.client.delete_context(&ctx_id).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);
        assert!(resp.body_bytes().is_empty());
    }));

    // 051_02_01: Delete @context with unknown identifier
    trials.push(trial(&format!("{PREFIX}::051_02_01::delete_unknown_context"), |ctx| async move {
        let resp = ctx.client.delete_context("urn:ngsi-ld:unknown:context:12345").await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
        let body = resp.json_value();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound");
    }));

    // 051_04_01: Delete wrong ID + reload=true → 400
    trials.push(trial(&format!("{PREFIX}::051_04_01::delete_with_wrong_id_and_reload_true"), |ctx| async move {
        let params = ContextQueryParams::new().reload(true);
        let resp = ctx.client.delete_context_with_params("urn:ngsi-ld:unknown:ctx", &params).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // 051_04_02: Delete wrong ID + reload=false → 404
    trials.push(trial(
        &format!("{PREFIX}::051_04_02::delete_with_wrong_id_and_reload_false"),
        |ctx| async move {
            let params = ContextQueryParams::new().reload(false);
            let resp = ctx.client.delete_context_with_params("urn:ngsi-ld:unknown:ctx", &params).await;
            assert_eq!(resp.status, StatusCode::NOT_FOUND);
        },
    ));

    // 051_04_03: Delete wrong ID + no reload → 404
    trials.push(trial(&format!("{PREFIX}::051_04_03::delete_with_wrong_id_no_reload"), |ctx| async move {
        let resp = ctx.client.delete_context("urn:ngsi-ld:unknown:ctx").await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // 051_04_04: Delete wrong ID + reload=xxx (invalid) → 400
    trials.push(trial(
        &format!("{PREFIX}::051_04_04::delete_with_wrong_id_and_reload_invalid"),
        |ctx| async move {
            let resp = ctx
                .client
                .request(Method::DELETE, "/jsonldContexts/urn:ngsi-ld:unknown:ctx?reload=xxx", None, Default::default())
                .await;
            assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        },
    ));

    // 051_04_05: Delete hosted + reload=true → 400
    trials.push(trial_clean_db(
        &format!("{PREFIX}::051_04_05::delete_hosted_with_reload_true"),
        |ctx| async move {
            let ctx_id = create_hosted_context(&ctx).await;

            let params = ContextQueryParams::new().reload(true);
            let resp = ctx.client.delete_context_with_params(&ctx_id, &params).await;
            assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        },
    ));

    // 051_06_01: Delete ImplicitlyCreated @context without reload param
    trials.push(trial_clean_db(
        &format!("{PREFIX}::051_06_01::delete_implicitly_created_context"),
        |ctx| async move {
            let (_sub_id, ctx_id) = create_implicit_context(&ctx).await;

            let resp = ctx.client.delete_context(&ctx_id).await;
            assert_eq!(resp.status, StatusCode::NO_CONTENT);
        },
    ));

    // 051_07_01: Delete ImplicitlyCreated @context with reload=true → 400
    trials.push(trial_clean_db(
        &format!("{PREFIX}::051_07_01::delete_implicitly_created_with_reload"),
        |ctx| async move {
            let (_sub_id, ctx_id) = create_implicit_context(&ctx).await;

            let params = ContextQueryParams::new().reload(true);
            let resp = ctx.client.delete_context_with_params(&ctx_id, &params).await;
            assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        },
    ));

    // 051_08_01: Delete core context without reload → 400
    trials.push(trial(&format!("{PREFIX}::051_08_01::delete_core_context_no_reload"), |ctx| async move {
        let core_context = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";
        let resp = ctx.client.delete_context(core_context).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        let body = resp.json_value();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
    }));

    // 051_03_01: Delete cached @context without reload param
    trials.push(trial_clean_db(&format!("{PREFIX}::051_03_01::delete_cached_context"), |ctx| async move {
        let (_server, ctx_url) = setup_cached_context(&ctx).await;

        // Verify it's cached
        let params = ContextQueryParams::new().kind("Cached").details(true);
        let resp = ctx.client.list_contexts_with_params(&params).await;
        let cached: Vec<serde_json::Value> = resp.json();
        let cached_ctx = cached
            .iter()
            .find(|c| c.get("URL").and_then(|u| u.as_str()).map_or(false, |u| u == ctx_url))
            .expect("context should be cached");
        let cached_id = cached_ctx["localId"].as_str().unwrap().to_string();

        // Delete the cached context
        let resp = ctx.client.delete_context(&cached_id).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);
        assert!(resp.body_bytes().is_empty());
    }));

    // 051_05_01: Delete cached @context with reload=true when server is unavailable → 503
    trials.push(trial_clean_db(
        &format!("{PREFIX}::051_05_01::delete_cached_context_reload_server_unavailable"),
        |ctx| async move {
            let (mut server, ctx_url) = setup_cached_context(&ctx).await;

            // Find cached context ID
            let params = ContextQueryParams::new().kind("Cached").details(true);
            let resp = ctx.client.list_contexts_with_params(&params).await;
            let cached: Vec<serde_json::Value> = resp.json();
            let cached_ctx = cached
                .iter()
                .find(|c| c.get("URL").and_then(|u| u.as_str()).map_or(false, |u| u == ctx_url))
                .expect("context should be cached");
            let cached_id = cached_ctx["localId"].as_str().unwrap().to_string();

            // Stop the context server
            server.stop();
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;

            // Attempt delete with reload=true — should fail since server is down
            let params = ContextQueryParams::new().reload(true);
            let resp = ctx.client.delete_context_with_params(&cached_id, &params).await;
            assert_eq!(resp.status, StatusCode::SERVICE_UNAVAILABLE);
            let body = resp.json_value();
            assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/LdContextNotAvailable");
        },
    ));

    // 051_08_02: Delete core context with reload=false → 400
    trials.push(trial(&format!("{PREFIX}::051_08_02::delete_core_context_reload_false"), |ctx| async move {
        let core_context = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";
        let params = ContextQueryParams::new().reload(false);
        let resp = ctx.client.delete_context_with_params(core_context, &params).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // 051_09_01: Delete core context with reload=true → 204, then verify re-downloaded
    trials.push(trial(&format!("{PREFIX}::051_09_01::delete_and_reload_core_context"), |ctx| async move {
        let core_context = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";

        // Verify it exists first
        let resp = ctx.client.get_context(core_context, &ContextQueryParams::new()).await;
        assert_eq!(resp.status, StatusCode::OK);

        // Delete with reload
        let params = ContextQueryParams::new().reload(true);
        let resp = ctx.client.delete_context_with_params(core_context, &params).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        // Verify still accessible after reload
        let resp = ctx.client.get_context(core_context, &ContextQueryParams::new()).await;
        assert_eq!(resp.status, StatusCode::OK);
    }));
}
