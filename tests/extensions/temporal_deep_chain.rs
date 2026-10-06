//! The temporal join over a chain of 1,000 and of 2,000 Linked Entities
//! (`temporal_linked_retrieval`).
//!
//! Clauses 5.7.3.4 and 5.7.4.4 do not admit Linked Entity retrieval on the
//! temporal operations, so the extension adds it, and clause 4.5.23 sets no cap
//! on `joinLevel` for the Core retrieval whose nesting it reuses. An inline
//! temporal join nests the Linked Entity under every in-window instance of the
//! Relationship that names it, so a chain of one Relationship instance per link
//! nests `depth` levels. The body is written and released as bytes, far deeper
//! than a recursive writer or destructor copes with on a worker thread, and is
//! checked as bytes too: it exceeds a JSON reader's recursion limit, so the
//! nesting is counted by scanning the text.
//!
//! These cases run only against a broker started with
//! `ANDROMEDA__EXTENSIONS__TEMPORAL_LINKED_RETRIEVAL=enabled`.

use andromeda_test_client::header::jsonld_link_header;
use andromeda_test_fixtures::context::{COMPOUND_CONTEXT, TEST_SUITE_CONTEXT};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The window every link of the chain was observed in.
const WINDOW: &str = "timerel=between&timeAt=2020-08-01T12:00:00Z&endTimeAt=2020-08-01T12:10:00Z";

/// The instant inside [`WINDOW`] every Relationship instance is observed at.
const OBSERVED_AT: &str = "2020-08-01T12:03:00Z";

/// The member a nested Linked Entity sits under (clause 4.5.23).
const NESTED_MEMBER: &[u8] = br#""entity":"#;

/// One link: it is parked in `target` at [`OBSERVED_AT`] when there is one, and
/// has a speed history of its own otherwise.
fn chain_link(id: &str, target: Option<&str>) -> Value {
    let mut link = json!({
        "id": id,
        "type": "Vehicle",
        "speed": [{"type": "Property", "value": 10, "observedAt": OBSERVED_AT}],
        "@context": [COMPOUND_CONTEXT]
    });
    if let Some(target) = target {
        link["isParkedIn"] = json!([{"type": "Relationship", "object": target, "observedAt": OBSERVED_AT}]);
    }
    link
}

/// Clause 6.4.3.2 as extended: a temporal query of the chain head with
/// `join=inline&joinLevel=depth` answers 200 with every link nested under an
/// `entity` member.
async fn assert_chain_answers_inline(depth: usize) {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated test environment");
    let client = scope.client();
    let ids: Vec<String> = (0..=depth).map(|index| scope.id(&format!("Vehicle:tdeep-{depth}-{index}"))).collect();
    // Created from the far end, so every link names an entity that already exists.
    for (index, id) in ids.iter().enumerate().rev() {
        let link = chain_link(id, ids.get(index + 1).map(String::as_str));
        let created = client.create_temporal_entity(&link).await.expect("create temporal entity request");
        assert_eq!(created.status, StatusCode::CREATED, "create link {index}: {:?}", created.body);
    }

    let path = format!(
        "temporal/entities?{}",
        scope.scope_query(&format!("type=Vehicle&{WINDOW}&id={}&join=inline&joinLevel={depth}", ids[0]))
    );
    let response = client
        .raw(Method::GET, &path)
        .header("Accept", "application/ld+json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"))
        .send()
        .await
        .expect("temporal query request");

    assert_eq!(response.status, StatusCode::OK, "the chain is answered, not refused: {:?}", response.body);
    let body = response.body.as_ref();
    let nested = body.windows(NESTED_MEMBER.len()).filter(|window| *window == NESTED_MEMBER).count();
    assert_eq!(nested, depth, "every link of the chain is nested under the temporal answer");
    let last = format!(r#""id":"{}""#, ids[depth]);
    assert!(
        body.windows(last.len()).any(|window| window == last.as_bytes()),
        "the last link of the chain is nested"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Follow-up 50: a 1,000-level inline temporal join answers.
#[tokio::test]
async fn temporal_join_inline_answers_a_1000_level_chain() {
    assert_chain_answers_inline(1000).await;
}

/// Follow-up 50: a 2,000-level inline temporal join answers.
#[tokio::test]
async fn temporal_join_inline_answers_a_2000_level_chain() {
    assert_chain_answers_inline(2000).await;
}
