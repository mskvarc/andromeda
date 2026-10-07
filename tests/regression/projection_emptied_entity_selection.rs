//! A projection that empties an Entity leaves it out of the selection, not out
//! of the page.
//!
//! ETSI GS CIM 009 v1.9.1 clauses 5.7.2.5 and 5.7.4.5 reduce "every Entity
//! within the payload body" to the Entity members a `pick` / `omit` projection
//! defines, and an Entity reduced to no member at all is not answered: a query
//! for `type=Building,Vehicle&pick=name` over a Vehicle that has no `name`
//! answers the Buildings alone. Dropping such an Entity after the page has been
//! cut and counted would make `NGSILD-Results-Count` (clause 6.3.13, "the total
//! number of matching results") count an Entity no page will ever carry, and a
//! page would fall short of its `limit` for a reason clause 5.5.9.1 does not
//! admit. The Entity is therefore not selected, on the Core and the Temporal
//! query alike, so the count and every page agree with the Entities returned.

use andromeda_test_client::{entity::results_count, header::jsonld_link_header, response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The temporal window clause 5.7.4.3 needs beside the projection under test,
/// on `modifiedAt`, which every instance a Core write recorded carries.
const WINDOW: &str = "timeproperty=modifiedAt&timerel=after&timeAt=2000-01-01T00:00:00Z";

/// The three Buildings every scenario reads, in identifier order: the middle
/// one carries a `category` and no `name`, so a projection onto `name` empties
/// it and nothing else.
struct Fixture {
    /// The Building before the emptied one.
    first: String,
    /// The Building the projection empties.
    emptied: String,
    /// The Building after the emptied one.
    last: String,
    /// The `idPattern` value that selects the three and nothing else.
    pattern: String,
}

/// Create the three Buildings of [`Fixture`].
async fn fixture(scope: &TestEnvironment) -> Fixture {
    let client = scope.client();
    let fixture = Fixture {
        first: scope.id("Building:emptied-selection-a"),
        emptied: scope.id("Building:emptied-selection-b"),
        last: scope.id("Building:emptied-selection-c"),
        pattern: format!("{}.%2A", scope.id("Building:emptied-selection-")),
    };
    for (id, attribute) in [(&fixture.first, "name"), (&fixture.emptied, "category"), (&fixture.last, "name")] {
        let entity = json!({
            "id": id,
            "type": "Building",
            attribute: {"type": "Property", "value": "Depot"},
            "@context": [TEST_SUITE_CONTEXT]
        });
        let created = client.post_entity(&entity).send().await.expect("create entity request");
        assert_eq!(created.status, StatusCode::CREATED, "create {id}: {:?}", created.body);
    }
    fixture
}

/// Send one query against a root-relative path.
async fn read(client: &TestClient, path: &str) -> TestResponse {
    client
        .raw(Method::GET, path)
        .header("Accept", "application/json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"))
        .send()
        .await
        .expect("query request")
}

/// Walk a counted query one entity per page, returning the count every page
/// reported and the identifiers in the order the pages carried them.
///
/// Every page before the last must be full: clause 5.5.9.1 lets a page fall
/// short of its `limit` only for its payload size.
async fn walk(client: &TestClient, base: &str, expected: usize) -> (Vec<Option<usize>>, Vec<String>) {
    let mut counts = Vec::new();
    let mut identifiers = Vec::new();
    for offset in 0..=expected {
        let response = read(client, &format!("{base}&count=true&limit=1&offset={offset}")).await;
        assert_eq!(response.status, StatusCode::OK, "{base} at {offset}: {:?}", response.body);
        counts.push(results_count(&response.headers));
        let body = response.json_value().expect("a 200 carries a JSON body");
        let page = body.as_array().expect("a query answers with an array");
        let expected_len = usize::from(offset < expected);
        assert_eq!(page.len(), expected_len, "{base} at {offset}: {body}");
        identifiers.extend(page.iter().filter_map(|entity| entity.get("id").and_then(Value::as_str).map(str::to_owned)));
    }
    (counts, identifiers)
}

/// Clause 5.7.2.5 with clause 6.3.13: `pick=name` selects the two Buildings
/// carrying a `name`, counts two, and serves them on two full pages.
#[tokio::test]
async fn regression_emptied_selection_a_query_counts_and_pages_only_what_the_pick_leaves() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let fixture = fixture(&scope).await;

    let base = format!("entities?type=Building&idPattern={}&pick=id,name", fixture.pattern);
    let (counts, identifiers) = walk(client, &base, 3).await;
    assert_eq!(counts, vec![Some(3); 4], "`pick=id` keeps every Entity a member");
    assert_eq!(identifiers, vec![fixture.first.clone(), fixture.emptied.clone(), fixture.last.clone()]);

    let base = format!("entities?type=Building&idPattern={}&pick=name", fixture.pattern);
    let (counts, identifiers) = walk(client, &base, 2).await;
    assert_eq!(counts, vec![Some(2); 3], "the emptied Building is not counted");
    assert!(identifiers.is_empty(), "`pick=name` leaves no `id` member: {identifiers:?}");

    scope.finish().await.expect("cleanup failed");
}

/// The `omit` twin: removing `id`, `type` and `category` empties the Building
/// that carries nothing else, and only that one.
#[tokio::test]
async fn regression_emptied_selection_a_query_counts_and_pages_only_what_the_omit_leaves() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let fixture = fixture(&scope).await;

    let base = format!("entities?type=Building&idPattern={}&omit=id,type,category", fixture.pattern);
    let (counts, _) = walk(client, &base, 2).await;
    assert_eq!(counts, vec![Some(2); 3], "the emptied Building is not counted");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.4.5 carries the same rule to the Temporal query: the Building
/// whose evolution holds no `name` is neither counted nor paged.
#[tokio::test]
async fn regression_emptied_selection_a_temporal_query_counts_and_pages_only_what_the_projection_leaves() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let fixture = fixture(&scope).await;

    let base = format!("temporal/entities?{WINDOW}&type=Building&idPattern={}&pick=id,name", fixture.pattern);
    let (counts, identifiers) = walk(client, &base, 3).await;
    assert_eq!(counts, vec![Some(3); 4], "`pick=id` keeps every Entity a member");
    assert_eq!(identifiers, vec![fixture.first.clone(), fixture.emptied.clone(), fixture.last.clone()]);

    let base = format!("temporal/entities?{WINDOW}&type=Building&idPattern={}&pick=name", fixture.pattern);
    let (counts, _) = walk(client, &base, 2).await;
    assert_eq!(counts, vec![Some(2); 3], "the emptied Building is not counted");

    let base = format!("temporal/entities?{WINDOW}&type=Building&idPattern={}&omit=id,type,category", fixture.pattern);
    let (counts, _) = walk(client, &base, 2).await;
    assert_eq!(counts, vec![Some(2); 3]);

    scope.finish().await.expect("cleanup failed");
}
