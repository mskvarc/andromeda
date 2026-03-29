use crate::harness::{client::RequestHeaders, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

const PREFIX: &str = "info::source_identity";

pub fn tests(trials: &mut Vec<Trial>) {
    // Returns 200 with identity
    trials.push(trial(&format!("{PREFIX}::returns_200_with_identity"), |ctx| async move {
        let resp = ctx.client.get_source_identity(Default::default()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert!(body["id"].as_str().is_some(), "response should have an id");
        assert_eq!(body["type"], "ContextSourceIdentity", "type should be ContextSourceIdentity");
    }));

    // Content negotiation: application/json
    trials.push(trial(&format!("{PREFIX}::content_negotiation_json"), |ctx| async move {
        let headers = RequestHeaders::new().accept("application/json");
        let resp = ctx.client.get_source_identity(headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let ct = resp.headers.get("content-type").expect("should have content-type");
        assert!(ct.to_str().unwrap().contains("application/json"), "content-type should be application/json");

        let body: serde_json::Value = resp.json();
        assert!(body.get("@context").is_none(), "application/json should not have @context in body");
        assert!(resp.headers.get("link").is_some(), "should have Link header for application/json");
    }));

    // Content negotiation: application/ld+json
    trials.push(trial(&format!("{PREFIX}::content_negotiation_ld_json"), |ctx| async move {
        let headers = RequestHeaders::new().accept_ld_json();
        let resp = ctx.client.get_source_identity(headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let ct = resp.headers.get("content-type").expect("should have content-type");
        assert!(
            ct.to_str().unwrap().contains("application/ld+json"),
            "content-type should be application/ld+json"
        );

        let body: serde_json::Value = resp.json();
        assert!(body.get("@context").is_some(), "application/ld+json should have @context in body");
    }));
}
