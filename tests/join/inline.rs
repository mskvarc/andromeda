//! `join=inline` cases: the linked entity nested under the Relationship that
//! names it, and the clause 4.21 sub-projection each nested copy carries.

use crate::{create, described_building, get_one, linked_building, query};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;

/// Clause 4.5.23: `join=inline` nests the linked entity under the
/// Relationship's `entity` member and leaves the response a single entity,
/// unlike `join=flat`, which turns it into a collection.
#[tokio::test]
async fn join_inline_nests_linked_entity_under_relationship() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_b = scope.id("Building:inl-B");
    let id_a = scope.id("Building:inl-A");
    create(client, &described_building(&id_b)).await;
    create(client, &linked_building(&id_a, &id_b)).await;

    let body = get_one(client, &id_a, "join=inline&joinLevel=1").await;

    assert!(body.is_object(), "join=inline on a single GET must return one object: {body}");
    assert_eq!(body["id"].as_str(), Some(id_a.as_str()));
    assert_eq!(body["linkedTo"]["entity"]["id"].as_str(), Some(id_b.as_str()));
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.21: the sub-projection a Relationship carries governs the copy of
/// the linked entity nested under that Relationship — clause 5.7.2.5 reducing
/// "every Entity within the payload body", so the nested terms decide the
/// nested copy's `id` and `type` as they decide its Attributes.
#[tokio::test]
async fn join_pick_linked_term_inline() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_b = scope.id("Building:inlpick-B");
    let id_a = scope.id("Building:inlpick-A");
    create(client, &described_building(&id_b)).await;
    create(client, &linked_building(&id_a, &id_b)).await;

    let body = query(
        client,
        &format!("id={id_a}&type=Building&join=inline&joinLevel=1&pick=linkedTo%7Bcategory,id%7D,id,type"),
    )
    .await;

    let root = &body.as_array().expect("query returns a collection")[0];
    assert_eq!(root["id"].as_str(), Some(id_a.as_str()));
    let nested = &root["linkedTo"]["entity"];
    assert_eq!(nested["id"].as_str(), Some(id_b.as_str()), "the nested pick names `id`: {nested}");
    assert!(nested.get("category").is_some(), "the nested pick keeps `category`: {nested}");
    assert!(nested.get("name").is_none(), "the nested pick drops `name`: {nested}");
    assert!(nested.get("type").is_none(), "the nested pick names no `type` keyword: {nested}");
    scope.finish().await.expect("cleanup failed");
}
