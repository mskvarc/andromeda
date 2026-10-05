//! `pick` / `omit` cases: the clause 6.3.15 structural-member rule on the
//! matching entities and the clause 4.21 sub-projections on the entities the
//! join reached.

use crate::{create, described_building, get_one, linked_building, member_with_id, query};
use andromeda_test_client::response::TestResponse;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

/// Clause 4.21: `pick=rel{a}` narrows the entity `rel` links to down to `a`,
/// while `rel` itself survives on the matching entity so the link remains.
///
/// Clause 5.7.2.5 reduces "every Entity within the payload body", the Linked
/// Entities included, so the nested terms govern the linked entity's `id` and
/// `type` as well: they survive because this `pick` names them, and the
/// companion case below is where they do not.
#[tokio::test]
async fn join_pick_linked_term_reduces_linked_entities() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_b = scope.id("Building:pick-B");
    let id_a = scope.id("Building:pick-A");
    create(client, &described_building(&id_b)).await;
    create(client, &linked_building(&id_a, &id_b)).await;

    let body = query(
        client,
        &format!("id={id_a}&type=Building&join=flat&joinLevel=1&pick=linkedTo%7Bcategory,id,type%7D,id,type"),
    )
    .await;

    let root = member_with_id(&body, &id_a);
    assert!(
        root.get("linkedTo").is_some(),
        "a term carrying a sub-projection keeps the relationship: {root}"
    );
    assert_eq!(root["type"].as_str(), Some("Building"));

    let linked = member_with_id(&body, &id_b);
    assert!(linked.get("category").is_some(), "the nested pick keeps `category`: {linked}");
    assert!(linked.get("name").is_none(), "the nested pick drops `name`: {linked}");
    assert_eq!(linked["type"].as_str(), Some("Building"), "the nested pick names `type`: {linked}");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.2.5, the other half of the rule above: a nested `pick` that
/// names no structural keyword leaves the Linked Entity with neither `id` nor
/// `type`, exactly as it leaves a matching entity with neither.
#[tokio::test]
async fn join_pick_linked_term_without_a_keyword_drops_the_linked_identity() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_b = scope.id("Building:pickbare-B");
    let id_a = scope.id("Building:pickbare-A");
    create(client, &described_building(&id_b)).await;
    create(client, &linked_building(&id_a, &id_b)).await;

    let body = query(client, &format!("id={id_a}&type=Building&join=flat&joinLevel=1&pick=linkedTo%7Bcategory%7D,id")).await;

    let members = body.as_array().expect("a flat join answers with a collection");
    assert_eq!(members.len(), 2, "the root and the entity it links to: {body}");
    let linked = members.iter().find(|member| member.get("id").is_none()).expect("the linked entity lost its id");
    assert!(linked.get("category").is_some(), "the nested pick keeps `category`: {linked}");
    assert!(linked.get("type").is_none(), "the nested pick names no `type` keyword: {linked}");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.15: a nested `omit` term drops the members it names from the
/// linked entity, and — carrying a sub-projection — leaves the Relationship
/// itself on the matching entity.
#[tokio::test]
async fn join_omit_linked_term_removes_nested_attribute() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_b = scope.id("Building:omit-B");
    let id_a = scope.id("Building:omit-A");
    create(client, &described_building(&id_b)).await;
    create(client, &linked_building(&id_a, &id_b)).await;

    let body = query(client, &format!("id={id_a}&type=Building&join=flat&joinLevel=1&omit=linkedTo%7Bname%7D")).await;

    let root = member_with_id(&body, &id_a);
    assert!(root.get("linkedTo").is_some(), "a nested omit term must not remove the relationship: {root}");

    let linked = member_with_id(&body, &id_b);
    assert!(linked.get("category").is_some(), "the nested omit keeps `category`: {linked}");
    assert!(linked.get("name").is_none(), "the nested omit drops `name`: {linked}");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.2.5 appends a flat Linked Entity once, "unless a target URI has
/// been previously encountered": the first encounter (breadth-first, in member
/// order) decides the list it is narrowed by, and a later Relationship reaching
/// the same target neither widens nor merges it.
#[tokio::test]
async fn join_pick_first_encounter_decides_a_shared_flat_target() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_b = scope.id("Building:first-B");
    let id_a = scope.id("Building:first-A");
    create(client, &described_building(&id_b)).await;
    create(
        client,
        &json!({
            "id": id_a,
            "type": "Building",
            "relA": {"type": "Relationship", "object": id_b},
            "relB": {"type": "Relationship", "object": id_b},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let body = query(
        client,
        &format!("id={id_a}&type=Building&join=flat&joinLevel=1&pick=relA%7Bcategory%7D,relB,id"),
    )
    .await;

    let members = body.as_array().expect("a flat join answers with a collection");
    assert_eq!(members.len(), 2, "the shared target is appended once: {body}");
    let linked = &members[1];
    assert!(linked.get("category").is_some(), "the first encounter's group keeps `category`: {linked}");
    assert!(linked.get("name").is_none(), "the later bare reference does not widen it: {linked}");
    assert!(linked.get("id").is_none(), "the first encounter's group names no `id`: {linked}");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.15 declares `pick` and `omit` mutually exclusive, on the query
/// binding and on the single-entity retrieval alike.
#[tokio::test]
async fn pick_and_omit_together_rejected() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:excl-A");
    create(client, &described_building(&id)).await;

    let queried = client
        .query_entities("type=Building&pick=category&omit=name", "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query_entities failed");
    assert_bad_request_data(&queried);

    let retrieved = client
        .get_entity(&id, "pick=category&omit=name", "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("get_entity failed");
    assert_bad_request_data(&retrieved);

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.15, read literally: `id`, `type` and `scope` are members a
/// projection names by keyword. A non-empty `pick` that names none of them
/// keeps none of them, and an `omit` drops exactly the ones it lists.
#[tokio::test]
async fn pick_structural_literal_semantics() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:struct-A");
    create(
        client,
        &json!({
            "id": id,
            "type": "Building",
            "scope": "/Madrid",
            "speed": {"type": "Property", "value": 42},
            "name": {"type": "Property", "value": "Depot"},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let picked = query(client, &format!("id={id}&type=Building&pick=speed")).await;
    let member = &picked.as_array().expect("query returns a collection")[0];
    assert!(member.get("speed").is_some(), "the picked attribute survives: {member}");
    assert!(member.get("name").is_none(), "an unpicked attribute is dropped: {member}");
    assert!(member.get("scope").is_none(), "a pick naming no structural keyword drops `scope`: {member}");
    assert!(member.get("id").is_none(), "a pick naming no structural keyword drops `id`: {member}");
    assert!(member.get("type").is_none(), "a pick naming no structural keyword drops `type`: {member}");

    let omitted = query(client, &format!("id={id}&type=Building&omit=type")).await;
    let member = &omitted.as_array().expect("query returns a collection")[0];
    assert_eq!(member["id"].as_str(), Some(id.as_str()), "omit=type leaves `id`: {member}");
    assert!(member.get("type").is_none(), "omit=type removes `type`: {member}");
    assert_eq!(member["scope"].as_str(), Some("/Madrid"), "omit=type leaves `scope`: {member}");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.15 governs `GET /entities/{id}` exactly as it governs the query
/// binding: the retrieval narrows to the picked members rather than ignoring
/// the parameter.
#[tokio::test]
async fn pick_on_single_entity_get_narrows_members() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:getpick-A");
    create(client, &described_building(&id)).await;

    let body = get_one(client, &id, "pick=category,id").await;
    assert_eq!(body["id"].as_str(), Some(id.as_str()));
    assert!(body.get("category").is_some(), "the picked attribute survives: {body}");
    assert!(body.get("name").is_none(), "an unpicked attribute is dropped: {body}");
    assert!(body.get("type").is_none(), "a pick that does not name `type` drops it: {body}");

    let omitted = get_one(client, &id, "omit=name").await;
    assert!(omitted.get("category").is_some(), "omit leaves the members it does not name: {omitted}");
    assert!(omitted.get("name").is_none(), "omit drops the attribute it names: {omitted}");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.21 reaches the single-entity retrieval too: an inline join there
/// narrows each nested copy by the sub-projection its Relationship carried.
#[tokio::test]
async fn pick_linked_term_on_single_entity_get_narrows_nested_entity() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_b = scope.id("Building:getnest-B");
    let id_a = scope.id("Building:getnest-A");
    create(client, &described_building(&id_b)).await;
    create(client, &linked_building(&id_a, &id_b)).await;

    let body = get_one(client, &id_a, "join=inline&joinLevel=1&pick=linkedTo%7Bcategory,id%7D,id").await;

    let nested = &body["linkedTo"]["entity"];
    assert_eq!(nested["id"].as_str(), Some(id_b.as_str()));
    assert!(nested.get("category").is_some(), "the nested pick keeps `category`: {nested}");
    assert!(nested.get("name").is_none(), "the nested pick drops `name`: {nested}");
    assert!(nested.get("type").is_none(), "the nested pick names no `type` keyword: {nested}");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.2.4: a `pick` term nesting into a Linked Entity names members
/// of an entity the response cannot carry when no join retrieves it. The
/// broker rejects the request rather than silently dropping the nesting.
#[tokio::test]
async fn nested_projection_without_a_join_returns_bad_request_data() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();

    let queried = client
        .query_entities("type=Building&pick=linkedTo%7Bcategory%7D", "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query_entities failed");
    assert_bad_request_data(&queried);

    let none_join = client
        .query_entities(
            "type=Building&join=%40none&pick=linkedTo%7Bcategory%7D",
            "application/ld+json",
            Some(TEST_SUITE_CONTEXT),
        )
        .await
        .expect("query_entities failed");
    assert_bad_request_data(&none_join);

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.2.4: the nesting may not reach past the traversal. Two brace
/// groups need two hops, and the same request becomes valid once `joinLevel`
/// covers them.
#[tokio::test]
async fn nested_projection_deeper_than_the_join_level_returns_bad_request_data() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:depth-A");
    create(client, &described_building(&id)).await;

    let too_deep = client
        .query_entities(
            &format!("id={id}&type=Building&join=flat&joinLevel=1&pick=a%7Bb%7Bc%7D%7D"),
            "application/ld+json",
            Some(TEST_SUITE_CONTEXT),
        )
        .await
        .expect("query_entities failed");
    assert_bad_request_data(&too_deep);

    let deep_enough = client
        .query_entities(
            &format!("id={id}&type=Building&join=flat&joinLevel=2&pick=a%7Bb%7Bc%7D%7D"),
            "application/ld+json",
            Some(TEST_SUITE_CONTEXT),
        )
        .await
        .expect("query_entities failed");
    assert_eq!(
        deep_enough.status,
        StatusCode::OK,
        "joinLevel=2 covers two brace groups: {:?}",
        deep_enough.body
    );

    let retrieved_too_deep = client
        .get_entity(
            &id,
            "join=flat&joinLevel=1&pick=a%7Bb%7Bc%7D%7D",
            "application/ld+json",
            Some(TEST_SUITE_CONTEXT),
        )
        .await
        .expect("get_entity failed");
    assert_bad_request_data(&retrieved_too_deep);

    scope.finish().await.expect("cleanup failed");
}

/// Assert a rejected request carries the clause 6.3.15 `BadRequestData`
/// problem type.
fn assert_bad_request_data(response: &TestResponse) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "body: {:?}", response.body);
    let body = response.json_value().expect("problem body must be valid JSON");
    assert!(
        body["type"].as_str().is_some_and(|problem_type| problem_type.ends_with("BadRequestData")),
        "problem type must end in BadRequestData: {body}"
    );
}
