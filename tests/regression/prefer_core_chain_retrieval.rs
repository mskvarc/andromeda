//! `Prefer: ngsi-ld=1.6` on a core retrieval whose inline join walks a chain of
//! 2,000 Linked Entities.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.5.23 sets no cap on `joinLevel`, so an inline
//! join nests as deep as the data goes, and clause 4.3.6.8 amends the payload
//! down to the version a client named in `Prefer: ngsi-ld=<version>` once it is
//! rendered. The amendment has to walk the same nesting the join produced, so a
//! chain far deeper than a recursive reader copes with still answers whole when
//! a version is named. Both the join and the amendment run iteratively; this
//! case is the Core retrieval half of the deep-chain coverage, the join suite
//! owning the same chain without a `Prefer` header.
//!
//! The body is checked as bytes, since a chain this deep exceeds a JSON reader's
//! recursion limit: the nesting is counted by walking the text iteratively.
//!
//! Needs the host broker: the case drives the HTTP API and is not run without
//! one.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::header::jsonld_link_header;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// Links nested below the root of the deepest chain.
const CHAIN: usize = 2000;

/// Links nested below the root of the shallower chain.
const SHALLOW_CHAIN: usize = 1000;

/// Entities per `entityOperations/create` request.
const PAGE: usize = 500;

/// The member a nested Linked Entity sits under (clause 4.5.23).
const NESTED_MEMBER: &[u8] = br#""entity":"#;

/// One chain entity: it names `target` through `linkedTo` when there is one.
fn chain_entity(id: &str, target: Option<&str>) -> Value {
    let mut entity = json!({"id": id, "type": "Building", "@context": [TEST_SUITE_CONTEXT]});
    if let Some(target) = target {
        entity["linkedTo"] = json!({"type": "Relationship", "object": target});
    }
    entity
}

/// Clauses 4.5.23 and 4.3.6.8: a retrieval of the chain head with
/// `join=inline&joinLevel=2000` answers the whole chain nested under `entity`
/// members when the client names version 1.6.
#[tokio::test]
async fn regression_prefer_1_6_core_retrieval_of_a_2000_chain_answers() {
    assert_chain_answers_under_prefer(CHAIN).await;
}

/// The same retrieval over a chain of 1,000 links.
#[tokio::test]
async fn regression_prefer_1_6_core_retrieval_of_a_1000_chain_answers() {
    assert_chain_answers_under_prefer(SHALLOW_CHAIN).await;
}

/// Retrieve the head of a chain of `depth` links with `join=inline` and
/// `Prefer: ngsi-ld=1.6`, and assert every link is nested.
async fn assert_chain_answers_under_prefer(depth: usize) {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids: Vec<String> = (0..=depth).map(|index| scope.id(&format!("Building:prefer-chain-{depth}-{index}"))).collect();
    let entities: Vec<Value> = ids
        .iter()
        .enumerate()
        .map(|(index, id)| chain_entity(id, ids.get(index + 1).map(String::as_str)))
        .collect();
    for page in entities.chunks(PAGE) {
        // clone: the request body owns its entities while the chunked fixture is borrowed.
        let created = client.batch_create(&Value::Array(page.to_vec())).await.expect("batch create request");
        assert_eq!(created.status, StatusCode::CREATED, "batch create: {:?}", created.body);
    }

    let response = client
        .raw(
            Method::GET,
            &format!("/ngsi-ld/v1/entities/{}?join=inline&joinLevel={depth}", urlencoding::encode(&ids[0])),
        )
        .header("Accept", "application/ld+json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"))
        .header("Prefer", "ngsi-ld=1.6")
        .send()
        .await
        .expect("retrieve chain head request");

    assert!(
        matches!(response.status, StatusCode::OK | StatusCode::NON_AUTHORITATIVE_INFORMATION),
        "the chain is answered, not refused: {} {:?}",
        response.status,
        response.body
    );
    let nested = response.body.windows(NESTED_MEMBER.len()).filter(|window| *window == NESTED_MEMBER).count();
    assert_eq!(nested, depth, "every link of the chain is nested under the amended payload");
    let last = format!(r#""id":"{}""#, ids[depth]);
    assert!(
        response.body.windows(last.len()).any(|window| window == last.as_bytes()),
        "the last link of the chain is nested"
    );

    scope.finish().await.expect("environment cleanup");
}
