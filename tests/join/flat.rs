//! `join=flat` traversal cases: the collection shape, the traversal depth and
//! the `containedBy` exclusion (NGSI-LD 4.5.23).

use crate::{create, get_one, ids_from_array, linked_building, minimal_building, post_query, query};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

#[tokio::test]
async fn join_flat_includes_directly_linked_entities() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_b = scope.id("Building:jf-B");
    let id_a = scope.id("Building:jf-A");
    create(client, &minimal_building(&id_b)).await;
    create(client, &linked_building(&id_a, &id_b)).await;

    let body = query(client, &format!("id={id_a}&type=Building&join=flat&joinLevel=1")).await;
    let ids = ids_from_array(&body);
    assert!(ids.contains(&id_a));
    assert!(ids.contains(&id_b));
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn join_flat_join_level_limits_traversal_depth() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_c = scope.id("Building:jl-C");
    let id_b = scope.id("Building:jl-B");
    let id_a = scope.id("Building:jl-A");

    create(client, &minimal_building(&id_c)).await;
    create(client, &linked_building(&id_b, &id_c)).await;
    create(client, &linked_building(&id_a, &id_b)).await;

    let body = query(client, &format!("id={id_a}&type=Building&join=flat&joinLevel=1")).await;
    let ids = ids_from_array(&body);
    assert!(ids.contains(&id_a));
    assert!(ids.contains(&id_b));
    assert!(!ids.contains(&id_c), "joinLevel=1 must exclude level-2 entity");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn join_flat_join_level_two_includes_two_hops() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_c = scope.id("Building:jl2-C");
    let id_b = scope.id("Building:jl2-B");
    let id_a = scope.id("Building:jl2-A");

    create(client, &minimal_building(&id_c)).await;
    create(client, &linked_building(&id_b, &id_c)).await;
    create(client, &linked_building(&id_a, &id_b)).await;

    let body = query(client, &format!("id={id_a}&type=Building&join=flat&joinLevel=2")).await;
    let ids = ids_from_array(&body);
    assert!(ids.contains(&id_a));
    assert!(ids.contains(&id_b));
    assert!(ids.contains(&id_c));
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn join_contained_by_excludes_specified_entity_from_join_results() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_c = scope.id("Building:cb-C");
    let id_b = scope.id("Building:cb-B");
    let id_a = scope.id("Building:cb-A");

    create(client, &minimal_building(&id_c)).await;
    create(client, &linked_building(&id_b, &id_c)).await;
    create(client, &linked_building(&id_a, &id_b)).await;

    let body = query(client, &format!("id={id_a}&type=Building&join=flat&joinLevel=2&containedBy={id_b}")).await;
    let ids = ids_from_array(&body);
    assert!(ids.contains(&id_a));
    assert!(!ids.contains(&id_b), "B must be excluded by containedBy");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn join_contained_by_multiple_ids_excludes_all_listed_entities() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_b = scope.id("Building:cbm-B");
    let id_c = scope.id("Building:cbm-C");
    let id_a = scope.id("Building:cbm-A");

    for id in [&id_b, &id_c] {
        create(client, &minimal_building(id)).await;
    }
    create(
        client,
        &json!({
            "id": id_a,
            "type": "Building",
            "linkedTo": {"type": "Relationship", "object": id_b},
            "alsoLinkedTo": {"type": "Relationship", "object": id_c},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let body = query(client, &format!("id={id_a}&type=Building&join=flat&joinLevel=1&containedBy={id_b},{id_c}")).await;
    let ids = ids_from_array(&body);
    assert!(ids.contains(&id_a));
    assert!(!ids.contains(&id_b));
    assert!(!ids.contains(&id_c));
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.23: `containedBy` names the entities the client already holds,
/// and a POST query carries it in its body exactly as the GET carries it in
/// its query string. The two bindings must reach the same traversal.
#[tokio::test]
async fn join_post_query_contained_by_excludes_specified_entity() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_c = scope.id("Building:pcb-C");
    let id_b = scope.id("Building:pcb-B");
    let id_a = scope.id("Building:pcb-A");

    create(client, &minimal_building(&id_c)).await;
    create(client, &linked_building(&id_b, &id_c)).await;
    create(client, &linked_building(&id_a, &id_b)).await;

    let unrestricted = post_query(
        client,
        &json!({
            "type": "Query",
            "entities": [{"id": id_a, "type": "Building"}],
            "join": "flat",
            "joinLevel": 2
        }),
    )
    .await;
    let unrestricted_ids = ids_from_array(&unrestricted);
    assert!(unrestricted_ids.contains(&id_a));
    assert!(unrestricted_ids.contains(&id_b));
    assert!(unrestricted_ids.contains(&id_c));

    let restricted = post_query(
        client,
        &json!({
            "type": "Query",
            "entities": [{"id": id_a, "type": "Building"}],
            "join": "flat",
            "joinLevel": 2,
            "containedBy": [id_b]
        }),
    )
    .await;
    let restricted_ids = ids_from_array(&restricted);
    assert!(restricted_ids.contains(&id_a));
    assert!(!restricted_ids.contains(&id_b), "a body `containedBy` must exclude B: {restricted_ids:?}");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn join_cycle_detection_does_not_infinite_loop() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_x = scope.id("Building:cyc-X");
    let id_y = scope.id("Building:cyc-Y");

    create(client, &linked_building(&id_x, &id_y)).await;
    create(client, &linked_building(&id_y, &id_x)).await;

    let body = query(client, &format!("id={id_x}&type=Building&join=flat&joinLevel=5")).await;
    let ids = ids_from_array(&body);
    assert!(ids.len() >= 2, "both entities must be returned despite cycle");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn join_single_entity_get_flat_join_returns_array_with_linked_entity() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_target = scope.id("Building:sg-target");
    let id_main = scope.id("Building:sg-main");
    create(client, &minimal_building(&id_target)).await;
    create(client, &linked_building(&id_main, &id_target)).await;

    let body = get_one(client, &id_main, "join=flat&joinLevel=1").await;
    let ids = ids_from_array(&body);
    assert!(ids.contains(&id_main));
    assert!(ids.contains(&id_target));
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn join_single_entity_get_with_contained_by_excludes_linked_entity() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_target = scope.id("Building:sgcb-target");
    let id_main = scope.id("Building:sgcb-main");
    create(client, &minimal_building(&id_target)).await;
    create(client, &linked_building(&id_main, &id_target)).await;

    let body = get_one(client, &id_main, &format!("join=flat&joinLevel=1&containedBy={id_target}")).await;
    let ids = ids_from_array(&body);
    assert!(ids.contains(&id_main));
    assert!(!ids.contains(&id_target));
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn join_join_level_zero_returns_bad_request_data() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let response = client
        .query_entities("type=Building&join=flat&joinLevel=0", "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query_entities failed");
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert!(
        response.json_value().expect("response body must be valid JSON")["type"]
            .as_str()
            .is_some_and(|problem_type| problem_type.ends_with("BadRequestData"))
    );
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.23: the flat rendering places the matching entities ahead of
/// the entities the traversal reached, and the traversal walks each root's
/// Relationships in the order the entity declares them. The order is a
/// property of the response, so it has to be identical across requests.
#[tokio::test]
async fn join_flat_appends_in_encounter_order() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_b = scope.id("Building:ord-B");
    let id_c = scope.id("Building:ord-C");
    let id_a = scope.id("Building:ord-A");

    create(client, &minimal_building(&id_b)).await;
    create(client, &minimal_building(&id_c)).await;
    create(
        client,
        &json!({
            "id": id_a,
            "type": "Building",
            "link1": {"type": "Relationship", "object": id_b},
            "link2": {"type": "Relationship", "object": id_c},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let expected = vec![id_a.clone(), id_b.clone(), id_c.clone()];
    for attempt in 0..2 {
        let body = query(client, &format!("id={id_a}&type=Building&join=flat&joinLevel=1")).await;
        assert_eq!(ids_from_array(&body), expected, "flat join order must be stable (attempt {attempt})");
    }
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.23 bounds the traversal by `joinLevel` alone: any positive depth
/// the client asks for is walked, with no broker-imposed ceiling of its own.
#[tokio::test]
async fn join_level_beyond_old_cap_resolves_deep_chain() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let chain: Vec<String> = ["A", "B", "C", "D", "E", "F", "G"]
        .iter()
        .map(|suffix| scope.id(&format!("Building:deep-{suffix}")))
        .collect();

    let last = chain.last().expect("chain is not empty");
    create(client, &minimal_building(last)).await;
    for window in chain.windows(2).rev() {
        create(client, &linked_building(&window[0], &window[1])).await;
    }

    let root = &chain[0];
    let body = query(client, &format!("id={root}&type=Building&join=flat&joinLevel=6")).await;
    let ids = ids_from_array(&body);
    for id in &chain {
        assert!(ids.contains(id), "joinLevel=6 must reach {id}: {ids:?}");
    }
    scope.finish().await.expect("cleanup failed");
}
