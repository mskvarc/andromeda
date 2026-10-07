//! A selection by type pages that type's Entities in identifier order, and
//! only those.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.17 selects an Entity whose types include
//! the one named ("possibly among others"), `,` selecting any of several and
//! `;` all of them, and clause 5.5.9 pages the result in a consistent order,
//! by `id` when none is requested. A type selection is answered from one row
//! per type an Entity carries, kept in step with every write, rather than by
//! walking every Entity in identifier order and filtering by type, which makes
//! a type whose identifiers sort after another type's pay for every Entity
//! before it. These tests pin what that index must never change: the selected
//! set, its order and its count, for a multi-typed Entity too, and after a
//! write moves an Entity between types.

use andromeda_test_client::{entity::results_count, header::jsonld_link_header, response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use regex::Regex;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::iter::once;

/// The Entities every scenario reads: six of an early-sorting type, five of a
/// late-sorting one, and one carrying both, whose identifier sorts between.
struct Fixture {
    /// The type of the `A:` Entities.
    early: String,
    /// The type of the `Z:` Entities.
    late: String,
    /// The `A:` Entities, in identifier order.
    early_ids: Vec<String>,
    /// The Entity carrying both types.
    both: String,
    /// The `Z:` Entities, in identifier order.
    late_ids: Vec<String>,
}

impl Fixture {
    /// Every Entity whose types include the late one, in identifier order.
    fn late_members(&self) -> Vec<String> {
        once(self.both.clone()).chain(self.late_ids.iter().cloned()).collect() // clone: the fixture keeps its identifiers for later scenarios
    }
}

/// Create the Entities of [`Fixture`].
async fn fixture(scope: &TestEnvironment) -> Fixture {
    let client = scope.client();
    let fixture = Fixture {
        early: scope.entity_type("MembershipEarly"),
        late: scope.entity_type("MembershipLate"),
        early_ids: (0..6).map(|n| scope.id(&format!("A:{n}"))).collect(),
        both: scope.id("M:0"),
        late_ids: (0..5).map(|n| scope.id(&format!("Z:{n}"))).collect(),
    };
    let typed = fixture
        .early_ids
        .iter()
        .map(|id| (id, json!(fixture.early)))
        .chain(once((&fixture.both, json!([fixture.early, fixture.late]))))
        .chain(fixture.late_ids.iter().map(|id| (id, json!(fixture.late))));
    for (id, types) in typed {
        let entity = json!({"id": id, "type": types, "name": {"type": "Property", "value": id}, "@context": [TEST_SUITE_CONTEXT]});
        let created = client.post_entity(&entity).send().await.expect("create entity request");
        assert_eq!(created.status, StatusCode::CREATED, "create {id}: {:?}", created.body);
    }
    fixture
}

/// Send one request against a root-relative path under the test-suite context.
async fn read(client: &TestClient, path: &str) -> TestResponse {
    client
        .raw(Method::GET, path)
        .header("Accept", "application/json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"))
        .send()
        .await
        .expect("query request")
}

/// The identifiers of a query page, in the order it carried them.
fn identifiers(response: &TestResponse) -> Vec<String> {
    let body = response.json_value().expect("a 200 carries a JSON body");
    body.as_array()
        .expect("a query answers with an array")
        .iter()
        .filter_map(|entity| entity.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

/// Walk `type=<selection>` two Entities per page by offset, returning the
/// count every page reported and the identifiers in page order.
async fn offset_walk(client: &TestClient, scope: &TestEnvironment, selection: &str, expected: usize) -> (Vec<Option<usize>>, Vec<String>) {
    let mut counts = Vec::new();
    let mut seen = Vec::new();
    for offset in (0..expected).step_by(2) {
        let path = scope.scope_query(&format!("entities?type={selection}&count=true&limit=2&offset={offset}"));
        let response = read(client, &path).await;
        assert_eq!(response.status, StatusCode::OK, "{path}: {:?}", response.body);
        counts.push(results_count(&response.headers));
        seen.extend(identifiers(&response));
    }
    (counts, seen)
}

/// Walk `type=<selection>` two Entities per page by the clause 5.5.9.1 cursor
/// the broker offers under `local=true`, following each `rel="next"` link.
async fn cursor_walk(client: &TestClient, scope: &TestEnvironment, selection: &str) -> Vec<String> {
    let next = Regex::new(r#"<([^>]+)>[^,]*?rel="next""#).expect("valid link-header pattern");
    let mut path = scope.scope_query(&format!("entities?type={selection}&limit=2&local=true"));
    let mut seen = Vec::new();
    for _ in 0..20 {
        let response = read(client, &path).await;
        assert_eq!(response.status, StatusCode::OK, "{path}: {:?}", response.body);
        seen.extend(identifiers(&response));
        // The `@context` and the pagination links may arrive as separate `Link`
        // header lines, so every line is read.
        let link = response
            .headers
            .get_all("link")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect::<Vec<_>>()
            .join(", ");
        match next.captures(&link) {
            Some(capture) => path = capture[1].to_owned(),
            None => return seen,
        }
    }
    // A walk that never ends returns what twenty pages carried, which no
    // caller's expectation matches.
    seen
}

/// Replace the Entity `id` (clause 5.6.18) with one carrying `types`.
async fn replace_types(client: &TestClient, id: &str, types: Value) {
    let entity = json!({"id": id, "type": types, "name": {"type": "Property", "value": "moved"}, "@context": [TEST_SUITE_CONTEXT]});
    let replaced = client
        .raw(Method::PUT, &format!("entities/{}", urlencoding::encode(id)))
        .header("Content-Type", "application/ld+json")
        .json(&entity)
        .send()
        .await
        .expect("replace entity request");
    assert_eq!(replaced.status, StatusCode::NO_CONTENT, "{:?}", replaced.body);
}

/// Clauses 4.17 and 5.5.9: a type whose identifiers sort after another
/// type's is paged in identifier order by offset and by cursor alike, with
/// the multi-typed Entity among its members, and counted exactly.
#[tokio::test]
async fn regression_type_membership_a_late_sorting_type_pages_in_identifier_order() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let fixture = fixture(&scope).await;

    let (counts, by_offset) = offset_walk(client, &scope, &fixture.late, 6).await;
    assert_eq!(counts, vec![Some(6); 3]);
    assert_eq!(by_offset, fixture.late_members());
    assert_eq!(cursor_walk(client, &scope, &fixture.late).await, fixture.late_members());

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.17: `,` selects an Entity carrying either type once, and `;`
/// selects only the Entity carrying both.
#[tokio::test]
async fn regression_type_membership_a_multi_typed_entity_is_selected_once_and_by_conjunction() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let fixture = fixture(&scope).await;

    let mut every: Vec<String> = fixture.early_ids.iter().cloned().chain(fixture.late_members()).collect(); // clone: the fixture keeps its identifiers
    every.sort();
    let (counts, seen) = offset_walk(client, &scope, &format!("{},{}", fixture.early, fixture.late), 12).await;
    assert_eq!(counts, vec![Some(12); 6]);
    assert_eq!(seen, every);

    let (counts, seen) = offset_walk(client, &scope, &format!("({};{})", fixture.early, fixture.late), 1).await;
    assert_eq!(counts, vec![Some(1)]);
    assert_eq!(seen, vec![fixture.both.clone()]);

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.2.33: selectors are a union, each holding its own type and
/// pattern together, so `{type: early, idPattern: A:0}` adds one early Entity
/// to every late one and not the other early ones.
#[tokio::test]
async fn regression_type_membership_several_selectors_keep_each_type_with_its_pattern() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let fixture = fixture(&scope).await;

    let body = json!({
        "type": "Query",
        "entities": [
            {"type": fixture.early, "idPattern": format!("^{}$", regex::escape(&fixture.early_ids[0]))},
            {"type": fixture.late, "idPattern": scope.id_pattern()}
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = client
        .raw(Method::POST, "entityOperations/query?count=true&limit=100")
        .header("Content-Type", "application/ld+json")
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await
        .expect("query request");
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let mut expected = vec![fixture.early_ids[0].clone()];
    expected.extend(fixture.late_members());
    expected.sort();
    assert_eq!(identifiers(&response), expected);
    assert_eq!(results_count(&response.headers), Some(expected.len()));

    scope.finish().await.expect("cleanup failed");
}

/// A write that drops a type moves the Entity out of that type's selection,
/// one that adds it back moves it in, and a deletion removes it everywhere.
#[tokio::test]
async fn regression_type_membership_follows_a_type_change_and_a_deletion() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let fixture = fixture(&scope).await;
    replace_types(client, &fixture.both, json!(fixture.early)).await;
    let (counts, seen) = offset_walk(client, &scope, &fixture.late, 5).await;
    assert_eq!(counts, vec![Some(5); 3]);
    assert_eq!(seen, fixture.late_ids);

    replace_types(client, &fixture.both, json!([fixture.late, fixture.early])).await;
    let (_, seen) = offset_walk(client, &scope, &fixture.late, 6).await;
    assert_eq!(seen, fixture.late_members());

    let path = format!("entities/{}", urlencoding::encode(&fixture.both));
    let deleted = client.raw(Method::DELETE, &path).send().await.expect("delete entity request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{:?}", deleted.body);
    let (counts, seen) = offset_walk(client, &scope, &fixture.early, 6).await;
    assert_eq!(counts, vec![Some(6); 3]);
    assert_eq!(seen, fixture.early_ids);

    scope.finish().await.expect("cleanup failed");
}
