use crate::harness::{
    client::ContextQueryParams,
    context_server::ContextServer,
    fixtures,
    trial::{trial, trial_clean_db},
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use std::collections::HashMap;

const PREFIX: &str = "jsonld_context::provision::add";

pub fn tests(trials: &mut Vec<Trial>) {
    // 050_01_01: Add valid hosted @context from key-value dictionary
    trials.push(trial_clean_db(
        &format!("{PREFIX}::050_01_01::add_valid_context_dictionary"),
        |ctx| async move {
            let body = fixtures::load_fragment("jsonld_context/@context-minimal-valid.json");

            let resp = ctx.client.create_context(&body).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            assert!(resp.body_bytes().is_empty(), "response body should be empty");
            let location = resp.location().expect("should have Location header");

            // Verify the context was created as Hosted
            let ctx_id = location.rsplit('/').next().unwrap();
            ctx.track_context(ctx_id);
            let params = ContextQueryParams::new().details(true);
            let resp = ctx.client.get_context(ctx_id, &params).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body = resp.json_value();
            assert_eq!(body["kind"], "Hosted");
        },
    ));

    // 050_01_02: Add valid hosted @context from URI list
    trials.push(trial_clean_db(&format!("{PREFIX}::050_01_02::add_valid_context_uri_list"), |ctx| async move {
        let body = fixtures::load_fragment("jsonld_context/@context-cached-valid.json");

        let resp = ctx.client.create_context(&body).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        assert!(resp.body_bytes().is_empty(), "response body should be empty");
        assert!(resp.location().is_some(), "should have Location header");

        let ctx_id = resp.location().unwrap().rsplit('/').next().unwrap().to_string();
        ctx.track_context(&ctx_id);
    }));

    // 050_02_01: Add @context with incorrect payload (missing @context key)
    trials.push(trial(
        &format!("{PREFIX}::050_02_01::add_invalid_context_incorrect_payload"),
        |ctx| async move {
            let body = fixtures::load_fragment("jsonld_context/@context-incorrect.json");

            let resp = ctx.client.create_context(&body).await;
            assert_eq!(resp.status, StatusCode::BAD_REQUEST);
            let body = resp.json_value();
            assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest");
        },
    ));

    // 050_02_02: Add @context with malformed JSON
    trials.push(trial(&format!("{PREFIX}::050_02_02::add_invalid_context_wrong_json"), |ctx| async move {
        let body = serde_json::json!({});

        let resp = ctx.client.create_context(&body).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // 050_03_01: ImplicitlyCreated @context via subscription creation
    trials.push(trial_clean_db(
        &format!("{PREFIX}::050_03_01::implicitly_created_context_via_subscription"),
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
            let sub_location = resp.location().unwrap().to_string();
            let sub_id = sub_location.rsplit('/').next().unwrap().to_string();
            ctx.track_subscription(&sub_id);

            // List contexts with kind=ImplicitlyCreated
            let params = ContextQueryParams::new().kind("ImplicitlyCreated");
            let resp = ctx.client.list_contexts_with_params(&params).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body: Vec<serde_json::Value> = resp.json();
            assert!(!body.is_empty(), "should have at least one ImplicitlyCreated context");
        },
    ));

    // 050_04_01: Add hosted @context with URI references, verify URIs become cached
    trials.push(trial_clean_db(
        &format!("{PREFIX}::050_04_01::add_hosted_context_with_cached_uri_refs"),
        |ctx| async move {
            // Start local context servers to serve the referenced URIs
            let ctx_content = r#"{"@context":{"X1":"urn:ngsi-ld:attributes:X1"}}"#;
            let server = ContextServer::start(HashMap::from([("/context.jsonld".to_string(), ctx_content.to_string())])).await;
            let ctx_url = server.url("/context.jsonld");

            // Create a hosted context that references the local server URL
            let body = serde_json::json!({
                "@context": [ctx_url]
            });
            let resp = ctx.client.create_context(&body).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            let hosted_uri = resp.location().unwrap().to_string();
            let hosted_id = hosted_uri.rsplit('/').next().unwrap().to_string();
            ctx.track_context(&hosted_id);

            // Verify the hosted context kind
            let params = ContextQueryParams::new().details(true);
            let resp = ctx.client.get_context(&hosted_id, &params).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body = resp.json_value();
            assert_eq!(body["kind"], "Hosted");

            // Create an entity using the hosted context to trigger caching of referenced URIs
            let entity_id = ctx.entity_id("urn:ngsi-ld:Testing");
            let entity = serde_json::json!({
                "id": &entity_id,
                "type": "Testing",
                "X1": {
                    "type": "Property",
                    "value": "test"
                },
                "@context": [&hosted_uri]
            });
            let resp = ctx.client.create_entity(&entity).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            // Verify the referenced URI is now cached
            let params = ContextQueryParams::new().kind("Cached");
            let resp = ctx.client.list_contexts_with_params(&params).await;
            assert_eq!(resp.status, StatusCode::OK);
            let cached: Vec<serde_json::Value> = resp.json();
            assert!(
                cached
                    .iter()
                    .any(|v| { v.as_str().map_or(false, |s| s.contains(&format!(":{}", server.port))) }),
                "referenced URI should be cached"
            );
        },
    ));
}
