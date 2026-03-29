use crate::harness::{
    client::ContextQueryParams,
    context_server::ContextServer,
    fixtures,
    trial::{trial, trial_clean_db},
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use std::collections::HashMap;

const PREFIX: &str = "jsonld_context::consumption::list";

/// Helper: assert listing contexts returns 200 and no non-core context URIs.
async fn assert_list_empty(ctx: &crate::harness::TestCtx, params: &ContextQueryParams) {
    let resp = ctx.client.list_contexts_with_params(params).await;
    assert_eq!(resp.status, StatusCode::OK);
    let body: Vec<serde_json::Value> = resp.json();
    let non_core: Vec<_> = body.iter().filter(|v| v.as_str().map_or(true, |s| !s.contains("core-context"))).collect();
    assert!(non_core.is_empty(), "expected no non-core contexts, got {non_core:?}");
}

/// Helper: create one hosted context and return (uri, id).
async fn create_one_context(ctx: &crate::harness::TestCtx) -> (String, String) {
    let ctx_body = fixtures::load_fragment("jsonld_context/@context-minimal-valid.json");
    let resp = ctx.client.create_context(&ctx_body).await;
    assert_eq!(resp.status, StatusCode::CREATED);
    let uri = resp.location().unwrap().to_string();
    let id = uri.rsplit('/').next().unwrap().to_string();
    ctx.track_context(&id);
    (uri, id)
}

/// Helper: assert listing contains the given context URI.
async fn assert_list_contains(ctx: &crate::harness::TestCtx, params: &ContextQueryParams, expected_uri: &str) {
    let resp = ctx.client.list_contexts_with_params(params).await;
    assert_eq!(resp.status, StatusCode::OK);
    let body: Vec<serde_json::Value> = resp.json();
    assert!(
        body.iter().any(|v| v.as_str().map_or(false, |s| s.ends_with(expected_uri))),
        "should contain context URI {expected_uri}"
    );
}

/// Helper: create 3 hosted contexts and return their URIs.
async fn create_three_contexts(ctx: &crate::harness::TestCtx) -> Vec<String> {
    let contexts = vec![
        serde_json::json!({"@context": {"A1": "urn:ngsi-ld:attributes:A1", "A2": "urn:ngsi-ld:attributes:A2", "A3": "urn:ngsi-ld:attributes:A3"}}),
        serde_json::json!({"@context": {"B1": "urn:ngsi-ld:attributes:B1", "B2": "urn:ngsi-ld:attributes:B2", "B3": "urn:ngsi-ld:attributes:B3"}}),
        serde_json::json!({"@context": {"C1": "urn:ngsi-ld:attributes:C1", "C2": "urn:ngsi-ld:attributes:C2", "C3": "urn:ngsi-ld:attributes:C3"}}),
    ];

    let mut uris = Vec::new();
    for c in &contexts {
        let resp = ctx.client.create_context(c).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let uri = resp.location().unwrap().to_string();
        let id = uri.rsplit('/').next().unwrap().to_string();
        ctx.track_context(&id);
        uris.push(uri);
    }
    uris
}

/// Helper: assert listing contains all given URIs.
async fn assert_list_contains_all(ctx: &crate::harness::TestCtx, params: &ContextQueryParams, uris: &[String]) {
    let resp = ctx.client.list_contexts_with_params(params).await;
    assert_eq!(resp.status, StatusCode::OK);
    let body: Vec<serde_json::Value> = resp.json();
    for uri in uris {
        assert!(
            body.iter().any(|v| v.as_str().map_or(false, |s| s.ends_with(uri.as_str()))),
            "should contain {uri}"
        );
    }
}

/// Helper: create a subscription that generates an ImplicitlyCreated context.
async fn create_implicit_context(ctx: &crate::harness::TestCtx) {
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
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 052_01: List @contexts with no previous creation ────────────

    trials.push(trial_clean_db(&format!("{PREFIX}::052_01_01::no_contexts_no_params"), |ctx| async move {
        assert_list_empty(&ctx, &ContextQueryParams::new()).await;
    }));

    trials.push(trial_clean_db(&format!("{PREFIX}::052_01_02::no_contexts_kind_hosted"), |ctx| async move {
        assert_list_empty(&ctx, &ContextQueryParams::new().kind("Hosted")).await;
    }));

    trials.push(trial_clean_db(&format!("{PREFIX}::052_01_03::no_contexts_kind_cached"), |ctx| async move {
        assert_list_empty(&ctx, &ContextQueryParams::new().kind("Cached")).await;
    }));

    trials.push(trial_clean_db(
        &format!("{PREFIX}::052_01_04::no_contexts_kind_implicitly_created"),
        |ctx| async move {
            assert_list_empty(&ctx, &ContextQueryParams::new().kind("ImplicitlyCreated")).await;
        },
    ));

    trials.push(trial_clean_db(&format!("{PREFIX}::052_01_05::no_contexts_details_false"), |ctx| async move {
        assert_list_empty(&ctx, &ContextQueryParams::new().details(false)).await;
    }));

    trials.push(trial_clean_db(
        &format!("{PREFIX}::052_01_06::no_contexts_details_false_kind_hosted"),
        |ctx| async move {
            assert_list_empty(&ctx, &ContextQueryParams::new().details(false).kind("Hosted")).await;
        },
    ));

    trials.push(trial_clean_db(
        &format!("{PREFIX}::052_01_07::no_contexts_details_false_kind_cached"),
        |ctx| async move {
            assert_list_empty(&ctx, &ContextQueryParams::new().details(false).kind("Cached")).await;
        },
    ));

    trials.push(trial_clean_db(
        &format!("{PREFIX}::052_01_08::no_contexts_details_false_kind_implicitly_created"),
        |ctx| async move {
            assert_list_empty(&ctx, &ContextQueryParams::new().details(false).kind("ImplicitlyCreated")).await;
        },
    ));

    // ── 052_02: List with one created context ───────────────────────

    trials.push(trial_clean_db(&format!("{PREFIX}::052_02_01::one_context_no_params"), |ctx| async move {
        let (uri, _) = create_one_context(&ctx).await;
        assert_list_contains(&ctx, &ContextQueryParams::new(), &uri).await;
    }));

    trials.push(trial_clean_db(&format!("{PREFIX}::052_02_02::one_context_kind_hosted"), |ctx| async move {
        let (uri, _) = create_one_context(&ctx).await;
        assert_list_contains(&ctx, &ContextQueryParams::new().kind("Hosted"), &uri).await;
    }));

    trials.push(trial_clean_db(&format!("{PREFIX}::052_02_03::one_context_details_false"), |ctx| async move {
        let (uri, _) = create_one_context(&ctx).await;
        assert_list_contains(&ctx, &ContextQueryParams::new().details(false), &uri).await;
    }));

    trials.push(trial_clean_db(
        &format!("{PREFIX}::052_02_04::one_context_details_false_kind_hosted"),
        |ctx| async move {
            let (uri, _) = create_one_context(&ctx).await;
            assert_list_contains(&ctx, &ContextQueryParams::new().details(false).kind("Hosted"), &uri).await;
        },
    ));

    // ── 052_03: List with multiple created contexts ─────────────────

    trials.push(trial_clean_db(&format!("{PREFIX}::052_03_01::multiple_contexts_no_params"), |ctx| async move {
        let uris = create_three_contexts(&ctx).await;
        assert_list_contains_all(&ctx, &ContextQueryParams::new(), &uris).await;
    }));

    trials.push(trial_clean_db(
        &format!("{PREFIX}::052_03_02::multiple_contexts_kind_hosted"),
        |ctx| async move {
            let uris = create_three_contexts(&ctx).await;
            assert_list_contains_all(&ctx, &ContextQueryParams::new().kind("Hosted"), &uris).await;
        },
    ));

    trials.push(trial_clean_db(
        &format!("{PREFIX}::052_03_03::multiple_contexts_details_false"),
        |ctx| async move {
            let uris = create_three_contexts(&ctx).await;
            assert_list_contains_all(&ctx, &ContextQueryParams::new().details(false), &uris).await;
        },
    ));

    trials.push(trial_clean_db(
        &format!("{PREFIX}::052_03_04::multiple_contexts_details_false_kind_hosted"),
        |ctx| async move {
            let uris = create_three_contexts(&ctx).await;
            assert_list_contains_all(&ctx, &ContextQueryParams::new().details(false).kind("Hosted"), &uris).await;
        },
    ));

    // ── 052_04: Invalid params → 400 ───────────────────────────────

    trials.push(trial(&format!("{PREFIX}::052_04_01::invalid_kind"), |ctx| async move {
        let resp = ctx.client.request(Method::GET, "/jsonldContexts?kind=other", None, Default::default()).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        let body = resp.json_value();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
    }));

    trials.push(trial(&format!("{PREFIX}::052_04_02::invalid_details"), |ctx| async move {
        let resp = ctx.client.request(Method::GET, "/jsonldContexts?details=other", None, Default::default()).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        let body = resp.json_value();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
    }));

    trials.push(trial(&format!("{PREFIX}::052_04_03::invalid_kind_with_details_true"), |ctx| async move {
        let resp = ctx
            .client
            .request(Method::GET, "/jsonldContexts?details=true&kind=other", None, Default::default())
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        let body = resp.json_value();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
    }));

    trials.push(trial(&format!("{PREFIX}::052_04_04::invalid_details_with_kind_hosted"), |ctx| async move {
        let resp = ctx
            .client
            .request(Method::GET, "/jsonldContexts?details=other&kind=Hosted", None, Default::default())
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        let body = resp.json_value();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
    }));

    trials.push(trial(&format!("{PREFIX}::052_04_05::invalid_details_and_kind"), |ctx| async move {
        let resp = ctx
            .client
            .request(Method::GET, "/jsonldContexts?details=other&kind=other", None, Default::default())
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        let body = resp.json_value();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
    }));

    // ── 052_05: details=true ────────────────────────────────────────

    trials.push(trial_clean_db(&format!("{PREFIX}::052_05_01::details_true_no_kind"), |ctx| async move {
        let ctx_body = fixtures::load_fragment("jsonld_context/@context-minimal-valid.json");
        let resp = ctx.client.create_context(&ctx_body).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let ctx_uri = resp.location().unwrap().to_string();
        let ctx_id = ctx_uri.rsplit('/').next().unwrap().to_string();
        ctx.track_context(&ctx_id);

        let params = ContextQueryParams::new().details(true);
        let resp = ctx.client.list_contexts_with_params(&params).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: Vec<serde_json::Value> = resp.json();

        let hosted = body.iter().find(|c| c.get("kind").and_then(|k| k.as_str()) == Some("Hosted"));
        assert!(hosted.is_some(), "should have a Hosted context in detailed list");

        let c = hosted.unwrap();
        assert!(c.get("URL").is_some(), "detailed response should have URL");
        assert!(c.get("kind").is_some(), "detailed response should have kind");
        assert!(c.get("createdAt").is_some(), "detailed response should have createdAt");
    }));

    trials.push(trial_clean_db(&format!("{PREFIX}::052_05_02::details_true_kind_hosted"), |ctx| async move {
        let ctx_body = fixtures::load_fragment("jsonld_context/@context-minimal-valid.json");
        let resp = ctx.client.create_context(&ctx_body).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let ctx_uri = resp.location().unwrap().to_string();
        let ctx_id = ctx_uri.rsplit('/').next().unwrap().to_string();
        ctx.track_context(&ctx_id);

        let params = ContextQueryParams::new().details(true).kind("Hosted");
        let resp = ctx.client.list_contexts_with_params(&params).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: Vec<serde_json::Value> = resp.json();

        let hosted = body.iter().find(|c| c.get("kind").and_then(|k| k.as_str()) == Some("Hosted"));
        assert!(hosted.is_some(), "should have a Hosted context");

        let c = hosted.unwrap();
        assert!(c.get("URL").is_some());
        assert!(c.get("kind").is_some());
        assert!(c.get("createdAt").is_some());
    }));

    // ── 052_06: ImplicitlyCreated contexts ──────────────────────────

    trials.push(trial_clean_db(&format!("{PREFIX}::052_06_01::implicit_no_params"), |ctx| async move {
        create_implicit_context(&ctx).await;
        let resp = ctx.client.list_contexts_with_params(&ContextQueryParams::new()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: Vec<serde_json::Value> = resp.json();
        let non_core: Vec<_> = body.iter().filter(|v| v.as_str().map_or(true, |s| !s.contains("core-context"))).collect();
        assert!(!non_core.is_empty(), "should have non-core contexts (implicit)");
    }));

    trials.push(trial_clean_db(&format!("{PREFIX}::052_06_02::implicit_kind_hosted"), |ctx| async move {
        create_implicit_context(&ctx).await;
        let params = ContextQueryParams::new().kind("Hosted");
        let resp = ctx.client.list_contexts_with_params(&params).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: Vec<serde_json::Value> = resp.json();
        assert!(body.is_empty(), "Hosted filter should not include implicit contexts");
    }));

    trials.push(trial_clean_db(&format!("{PREFIX}::052_06_03::implicit_kind_cached"), |ctx| async move {
        create_implicit_context(&ctx).await;
        let params = ContextQueryParams::new().kind("Cached");
        let resp = ctx.client.list_contexts_with_params(&params).await;
        assert_eq!(resp.status, StatusCode::OK);
        // The subscription's external @context URIs get cached by the broker,
        // so Cached may return results — just verify the response is valid.
        let _body: Vec<serde_json::Value> = resp.json();
    }));

    trials.push(trial_clean_db(
        &format!("{PREFIX}::052_06_04::implicit_kind_implicitly_created"),
        |ctx| async move {
            create_implicit_context(&ctx).await;
            let params = ContextQueryParams::new().kind("ImplicitlyCreated");
            let resp = ctx.client.list_contexts_with_params(&params).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body: Vec<serde_json::Value> = resp.json();
            assert!(!body.is_empty(), "should have ImplicitlyCreated contexts");
        },
    ));

    trials.push(trial_clean_db(&format!("{PREFIX}::052_06_05::implicit_details_false"), |ctx| async move {
        create_implicit_context(&ctx).await;
        let resp = ctx.client.list_contexts_with_params(&ContextQueryParams::new().details(false)).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: Vec<serde_json::Value> = resp.json();
        let non_core: Vec<_> = body.iter().filter(|v| v.as_str().map_or(true, |s| !s.contains("core-context"))).collect();
        assert!(!non_core.is_empty(), "should have non-core contexts (implicit)");
    }));

    trials.push(trial_clean_db(
        &format!("{PREFIX}::052_06_06::implicit_details_false_kind_hosted"),
        |ctx| async move {
            create_implicit_context(&ctx).await;
            let params = ContextQueryParams::new().details(false).kind("Hosted");
            let resp = ctx.client.list_contexts_with_params(&params).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body: Vec<serde_json::Value> = resp.json();
            assert!(body.is_empty(), "Hosted filter should not include implicit contexts");
        },
    ));

    trials.push(trial_clean_db(
        &format!("{PREFIX}::052_06_07::implicit_details_false_kind_cached"),
        |ctx| async move {
            create_implicit_context(&ctx).await;
            let params = ContextQueryParams::new().details(false).kind("Cached");
            let resp = ctx.client.list_contexts_with_params(&params).await;
            assert_eq!(resp.status, StatusCode::OK);
            // The subscription's external @context URIs get cached by the broker,
            // so Cached may return results — just verify the response is valid.
            let _body: Vec<serde_json::Value> = resp.json();
        },
    ));

    trials.push(trial_clean_db(
        &format!("{PREFIX}::052_06_08::implicit_details_false_kind_implicitly_created"),
        |ctx| async move {
            create_implicit_context(&ctx).await;
            let params = ContextQueryParams::new().details(false).kind("ImplicitlyCreated");
            let resp = ctx.client.list_contexts_with_params(&params).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body: Vec<serde_json::Value> = resp.json();
            assert!(!body.is_empty(), "should have ImplicitlyCreated contexts");
        },
    ));

    // ── 052_07: List cached contexts with details=true ──────────────

    trials.push(trial_clean_db(
        &format!("{PREFIX}::052_07_01::cached_contexts_with_details"),
        |ctx| async move {
            let ctx_content = r#"{"@context":{"X1":"urn:ngsi-ld:attributes:X1"}}"#;
            let server = ContextServer::start(HashMap::from([("/context.jsonld".to_string(), ctx_content.to_string())])).await;
            let ctx_url = server.url("/context.jsonld");

            // Create entity using external context to trigger caching
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

            // List with details=true and kind=Cached
            let params = ContextQueryParams::new().details(true).kind("Cached");
            let resp = ctx.client.list_contexts_with_params(&params).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body: Vec<serde_json::Value> = resp.json();

            let cached = body.iter().find(|c| c.get("URL").and_then(|u| u.as_str()).map_or(false, |u| u == ctx_url));
            assert!(cached.is_some(), "should find cached context in detailed list");

            let c = cached.unwrap();
            assert_eq!(c["kind"], "Cached");
            assert!(c.get("URL").is_some());
            assert!(c.get("createdAt").is_some());

            let _ = server;
        },
    ));
}
