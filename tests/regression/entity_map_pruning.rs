//! What an `EntityMap` fixes, and what a prepared result page sheds from it.
//!
//! Clause 4.3.6.7 filters the entities an `EntityMap` lists before they join a
//! result page and removes "identifiers of Entities not matching the filters at
//! the time of checking" from the map; clause 5.5.9.3 repeats it for the
//! candidate entities of a paginated map-scoped query, and clause 5.5.14 puts
//! the duty on the broker that created the map. The pruning tests here reuse a
//! map across a paged sequence, make one entity stop fitting the query in
//! between, and assert both that the entity does not reach a later page and
//! that its identifier has left the map resource.
//!
//! The scoping tests assert the other half of clause 5.5.14 — "queries shall be
//! filtered to only consider the Entities listed [...] An `EntityMap` fixes the
//! Entities to be considered for subsequent requests based on it" — which a
//! request under a map can violate in either direction: by answering with
//! entities the map does not list, or by shedding entities it was never asked
//! about.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{header::jsonld_link_header, response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{
    callback_path::CallbackPath,
    callback_server::MockResponse,
    context_source_server::ContextSourceServer,
    server_config::MockServerConfig,
};
use axum::{
    body::Bytes,
    http::{HeaderMap, HeaderValue, header::CONTENT_TYPE},
};
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::iter::once;

/// The current-entity query endpoint.
const ENTITIES: &str = "/ngsi-ld/v1/entities";
/// The endpoint that creates an `EntityMap` over a current-entity query.
const ENTITY_MAPS: &str = "/ngsi-ld/v1/entityMaps";
/// The temporal query endpoint.
const TEMPORAL_ENTITIES: &str = "/ngsi-ld/v1/temporal/entities";
/// The endpoint that creates an `EntityMap` over a temporal query (clause 5.14.5).
const TEMPORAL_ENTITY_MAPS: &str = "/ngsi-ld/v1/temporal/entityMaps";
/// A temporal window wide enough to cover every recorded instance.
const WHOLE_HISTORY: &str = "timerel=before&timeAt=2099-01-01T00:00:00Z";

/// Send a `GET` under the test-suite context, optionally scoped to an `EntityMap`.
async fn get_scoped(client: &TestClient, path: &str, query: &str, map_id: Option<&str>) -> TestResponse {
    let mut request = client
        .raw(Method::GET, &format!("{path}?{query}"))
        .header("Accept", "application/ld+json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"));
    if let Some(map_id) = map_id {
        request = request.header("NGSILD-EntityMap", map_id);
    }
    request.send().await.expect("map-scoped GET failed")
}

/// Create an entity and assert the broker accepted it.
async fn create(client: &TestClient, body: &Value) {
    let response = client.post_entity(body).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED, "create entity: {:?}", response.body);
}

/// Replace an entity in full (clause 5.6.19), which is how a test makes one
/// stop fitting the query it was mapped under.
async fn replace(client: &TestClient, id: &str, body: &Value) {
    let response = client
        .raw(Method::PUT, &format!("{ENTITIES}/{}", urlencoding::encode(id)))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("replace entity failed");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "replace entity: {:?}", response.body);
}

/// Retrieve one entity under an `EntityMap` (clause 5.7.1.4).
async fn retrieve_scoped(client: &TestClient, id: &str, map_id: &str) -> TestResponse {
    client
        .raw(Method::GET, &format!("{ENTITIES}/{}", urlencoding::encode(id)))
        .header("Accept", "application/ld+json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"))
        .header("NGSILD-EntityMap", map_id)
        .send()
        .await
        .expect("map-scoped retrieval failed")
}

/// Run a query that creates its own `EntityMap` (Table 6.4.3.2-1 `entityMap`)
/// and answer with the identifier of the map it created.
///
/// Unlike the `entityMaps` resource, a minting query records what every context
/// source returned entity by entity (clause 5.2.39), which is what a test about
/// the sources of a mapped entity needs.
async fn mint_map_through_query(client: &TestClient, query: &str) -> String {
    let response = get_scoped(client, ENTITIES, &format!("{query}&entityMap=true"), None).await;
    assert_eq!(response.status, StatusCode::CREATED, "minting query: {:?}", response.body);
    response
        .headers
        .get("NGSILD-EntityMap")
        .and_then(|value| value.to_str().ok())
        .expect("a minting query answers with the location of its map")
        .to_owned()
}

/// Create an `EntityMap` over `query` and answer with its identifier.
async fn create_map(client: &TestClient, path: &str, query: &str) -> String {
    let response = get_scoped(client, path, query, None).await;
    assert_eq!(response.status, StatusCode::CREATED, "create EntityMap: {:?}", response.body);
    let body = response.json_value().expect("EntityMap body must be JSON");
    body["id"].as_str().expect("the created EntityMap must carry an id").to_owned()
}

/// The identifiers a query answered with, in response order.
fn ids_of(response: &TestResponse) -> Vec<String> {
    response
        .json_value()
        .expect("query body must be JSON")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entity| entity.get("id").and_then(Value::as_str).map(String::from))
        .collect()
}

/// The identifiers an `EntityMap` resource still lists (Table 5.2.39-2 `entityMap`).
async fn mapped_ids(client: &TestClient, map_id: &str) -> Vec<String> {
    let response = client.retrieve_entity_map(map_id).await.expect("retrieve EntityMap failed");
    assert_eq!(response.status, StatusCode::OK, "retrieve EntityMap: {:?}", response.body);
    response.json_value().expect("EntityMap body must be JSON")["entityMap"]
        .as_object()
        .expect("an EntityMap renders its entityMap member")
        .keys()
        .cloned()
        .collect()
}

/// The comma-joined, percent-encoded `id` filter that keeps a query to the
/// entities one isolated test owns.
fn id_filter(ids: &[&str]) -> String {
    ids.iter().map(|id| urlencoding::encode(id).into_owned()).collect::<Vec<_>>().join(",")
}

/// An identifier the first page did not carry, so the entity broken next is one
/// a later page would otherwise have offered.
fn absent_from<'a>(page: &[String], candidates: &'a [String]) -> &'a str {
    candidates
        .iter()
        .find(|id| !page.contains(id))
        .expect("with four mapped entities a page of two leaves one behind")
        .as_str()
}

/// Clause 4.3.6.7: "Filtering based on queries ... as provided in the original
/// query request, is applied to the Entities before adding the Entity to a
/// result page, i.e. identifiers of Entities not matching the filters at the
/// time of checking are removed from the Entity map." An entity whose value
/// drops out of the `q` filter between two pages of the same map-scoped
/// sequence must therefore neither reach the later page nor stay in the map.
#[tokio::test]
async fn regression_entity_map_pruning_q_filter_sheds_an_entity_that_stopped_matching() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let ids: Vec<String> = (1..=4).map(|index| scope.id(&format!("Building:em-prune-q-{index}"))).collect();
    for id in &ids {
        create(
            client,
            &json!({
                "id": id,
                "type": "Building",
                "temperature": {"type": "Property", "value": 30.0},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
    }

    let filter = id_filter(&ids.iter().map(String::as_str).collect::<Vec<_>>());
    let map_id = create_map(client, ENTITY_MAPS, &format!("type=Building&id={filter}&q=temperature%3E20")).await;
    assert_eq!(mapped_ids(client, &map_id).await.len(), 4, "the created map fixes all four matching entities");

    let first_page = get_scoped(client, ENTITIES, "type=Building&q=temperature%3E20&limit=2&offset=0", Some(&map_id)).await;
    assert_eq!(first_page.status, StatusCode::OK, "first page: {:?}", first_page.body);
    let first_ids = ids_of(&first_page);
    assert_eq!(first_ids.len(), 2, "the first page is filled to the maximum (clause 5.5.9.3)");

    let victim = absent_from(&first_ids, &ids).to_owned();
    replace(
        client,
        &victim,
        &json!({
            "type": "Building",
            "temperature": {"type": "Property", "value": 1.0},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let later_page = get_scoped(client, ENTITIES, "type=Building&q=temperature%3E20&limit=100&offset=0", Some(&map_id)).await;
    assert_eq!(later_page.status, StatusCode::OK, "later page: {:?}", later_page.body);
    let later_ids = ids_of(&later_page);
    assert!(
        !later_ids.contains(&victim),
        "an entity that no longer matches the query must not reach a later page (clause 4.3.6.7): {later_ids:?}"
    );

    let remaining = mapped_ids(client, &map_id).await;
    assert!(
        !remaining.contains(&victim),
        "clause 4.3.6.7 removes the identifier of an entity found not matching at the time of checking: {remaining:?}"
    );
    assert_eq!(remaining.len(), 3, "only the entity that stopped matching leaves the map: {remaining:?}");

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.3.6.7 names scope queries among the filters reapplied while a page
/// is prepared, and clause 5.5.9.3 requires the entities that no longer fit to
/// be removed from the map during pagination. An entity moved out of the mapped
/// scope must therefore leave both the later page and the map.
#[tokio::test]
async fn regression_entity_map_pruning_scope_query_sheds_an_entity_moved_out_of_scope() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let root = format!("T{}", scope.id_prefix().chars().filter(char::is_ascii_alphanumeric).collect::<String>());
    let mapped_scope = format!("/{root}/AreaA");

    let ids: Vec<String> = (1..=4).map(|index| scope.id(&format!("Building:em-prune-scope-{index}"))).collect();
    for id in &ids {
        create(
            client,
            &json!({
                "id": id,
                "type": "Building",
                "scope": mapped_scope,
                "temperature": {"type": "Property", "value": 30.0},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
    }

    let filter = id_filter(&ids.iter().map(String::as_str).collect::<Vec<_>>());
    let encoded_scope = urlencoding::encode(&mapped_scope).into_owned();
    let map_id = create_map(client, ENTITY_MAPS, &format!("type=Building&id={filter}&scopeQ={encoded_scope}")).await;
    assert_eq!(mapped_ids(client, &map_id).await.len(), 4, "the created map fixes all four scoped entities");

    let scoped_query = format!("type=Building&scopeQ={encoded_scope}");
    let first_page = get_scoped(client, ENTITIES, &format!("{scoped_query}&limit=2&offset=0"), Some(&map_id)).await;
    assert_eq!(first_page.status, StatusCode::OK, "first page: {:?}", first_page.body);
    let first_ids = ids_of(&first_page);
    assert_eq!(first_ids.len(), 2, "the first page is filled to the maximum (clause 5.5.9.3)");

    let victim = absent_from(&first_ids, &ids).to_owned();
    replace(
        client,
        &victim,
        &json!({
            "type": "Building",
            "scope": format!("/{root}/AreaB"),
            "temperature": {"type": "Property", "value": 30.0},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let later_page = get_scoped(client, ENTITIES, &format!("{scoped_query}&limit=100&offset=0"), Some(&map_id)).await;
    assert_eq!(later_page.status, StatusCode::OK, "later page: {:?}", later_page.body);
    let later_ids = ids_of(&later_page);
    assert!(
        !later_ids.contains(&victim),
        "an entity outside the mapped scope must not reach a later page (clause 4.3.6.7): {later_ids:?}"
    );

    let remaining = mapped_ids(client, &map_id).await;
    assert!(
        !remaining.contains(&victim),
        "clause 5.5.9.3 removes an entity no longer fitting the scope query from the map: {remaining:?}"
    );
    assert_eq!(remaining.len(), 3, "only the entity that left the scope leaves the map: {remaining:?}");

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.3.6.7 names geoqueries among the filters reapplied before an entity
/// joins a result page. An entity moved outside the mapped `near` circle must
/// leave both the later page and the map.
#[tokio::test]
async fn regression_entity_map_pruning_geoquery_sheds_an_entity_that_moved_away() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let ids: Vec<String> = (1..=4).map(|index| scope.id(&format!("Building:em-prune-geo-{index}"))).collect();
    for id in &ids {
        create(
            client,
            &json!({
                "id": id,
                "type": "Building",
                "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52]}},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
    }

    let filter = id_filter(&ids.iter().map(String::as_str).collect::<Vec<_>>());
    let geo_query = "georel=near;maxDistance==15000&geometry=Point&coordinates=%5B13.388930,52.534473%5D";
    let map_id = create_map(client, ENTITY_MAPS, &format!("type=Building&id={filter}&{geo_query}")).await;
    assert_eq!(mapped_ids(client, &map_id).await.len(), 4, "the created map fixes all four nearby entities");

    let first_page = get_scoped(client, ENTITIES, &format!("type=Building&{geo_query}&limit=2&offset=0"), Some(&map_id)).await;
    assert_eq!(first_page.status, StatusCode::OK, "first page: {:?}", first_page.body);
    let first_ids = ids_of(&first_page);
    assert_eq!(first_ids.len(), 2, "the first page is filled to the maximum (clause 5.5.9.3)");

    let victim = absent_from(&first_ids, &ids).to_owned();
    replace(
        client,
        &victim,
        &json!({
            "type": "Building",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [20.0, 40.0]}},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let later_page = get_scoped(client, ENTITIES, &format!("type=Building&{geo_query}&limit=100&offset=0"), Some(&map_id)).await;
    assert_eq!(later_page.status, StatusCode::OK, "later page: {:?}", later_page.body);
    let later_ids = ids_of(&later_page);
    assert!(
        !later_ids.contains(&victim),
        "an entity outside the geoquery must not reach a later page (clause 4.3.6.7): {later_ids:?}"
    );

    let remaining = mapped_ids(client, &map_id).await;
    assert!(
        !remaining.contains(&victim),
        "clause 4.3.6.7 removes an entity that no longer satisfies the geoquery: {remaining:?}"
    );
    assert_eq!(remaining.len(), 3, "only the entity that moved away leaves the map: {remaining:?}");

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.9.3: "Entities not or no longer fitting the query shall be
/// removed from the Entity map during pagination." An entity deleted in the
/// middle of a map-scoped paging sequence no longer fits, so its identifier
/// must not survive in the map either.
#[tokio::test]
async fn regression_entity_map_pruning_deleted_entity_leaves_the_map() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let ids: Vec<String> = (1..=4).map(|index| scope.id(&format!("Building:em-prune-del-{index}"))).collect();
    for id in &ids {
        create(
            client,
            &json!({
                "id": id,
                "type": "Building",
                "temperature": {"type": "Property", "value": 30.0},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
    }

    let filter = id_filter(&ids.iter().map(String::as_str).collect::<Vec<_>>());
    let map_id = create_map(client, ENTITY_MAPS, &format!("type=Building&id={filter}")).await;
    assert_eq!(mapped_ids(client, &map_id).await.len(), 4, "the created map fixes all four entities");

    let first_page = get_scoped(client, ENTITIES, "type=Building&limit=2&offset=0", Some(&map_id)).await;
    assert_eq!(first_page.status, StatusCode::OK, "first page: {:?}", first_page.body);
    let first_ids = ids_of(&first_page);
    assert_eq!(first_ids.len(), 2, "the first page is filled to the maximum (clause 5.5.9.3)");

    let victim = absent_from(&first_ids, &ids).to_owned();
    let deleted = client.delete_entity(&victim).await.expect("delete entity failed");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete entity: {:?}", deleted.body);

    let later_page = get_scoped(client, ENTITIES, "type=Building&limit=100&offset=0", Some(&map_id)).await;
    assert_eq!(later_page.status, StatusCode::OK, "later page: {:?}", later_page.body);
    let later_ids = ids_of(&later_page);
    assert!(
        !later_ids.contains(&victim),
        "a deleted entity must not reach a later page of the same map (clause 5.5.9.3): {later_ids:?}"
    );

    let remaining = mapped_ids(client, &map_id).await;
    assert!(
        !remaining.contains(&victim),
        "clause 5.5.14 makes the creating broker remove entities that no longer match at the time of processing: {remaining:?}"
    );
    assert_eq!(remaining.len(), 3, "only the deleted entity leaves the map: {remaining:?}");

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.14.5 creates an `EntityMap` for a Query Temporal Evolution of
/// Entities request, and clause 4.3.6.7 covers that operation with the same
/// rule: the entities of the map are rechecked while a page is prepared and the
/// ones that no longer match are removed. An entity whose temporal
/// representation is deleted mid-sequence must therefore leave the map too.
#[tokio::test]
async fn regression_entity_map_pruning_temporal_query_sheds_a_deleted_history() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let ids: Vec<String> = (1..=4).map(|index| scope.id(&format!("Building:em-prune-temporal-{index}"))).collect();
    for id in &ids {
        create(
            client,
            &json!({
                "id": id,
                "type": "Building",
                "temperature": {"type": "Property", "value": 30.0, "observedAt": "2024-01-01T00:00:00Z"},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
    }

    let filter = id_filter(&ids.iter().map(String::as_str).collect::<Vec<_>>());
    let map_id = create_map(client, TEMPORAL_ENTITY_MAPS, &format!("type=Building&id={filter}&{WHOLE_HISTORY}")).await;
    assert_eq!(mapped_ids(client, &map_id).await.len(), 4, "the created temporal map fixes all four histories");

    let first_page = get_scoped(
        client,
        TEMPORAL_ENTITIES,
        &format!("type=Building&{WHOLE_HISTORY}&limit=2&offset=0"),
        Some(&map_id),
    )
    .await;
    assert_eq!(first_page.status, StatusCode::OK, "first temporal page: {:?}", first_page.body);
    let first_ids = ids_of(&first_page);
    assert_eq!(first_ids.len(), 2, "the first page is filled to the maximum (clause 5.5.9.3)");

    let victim = absent_from(&first_ids, &ids).to_owned();
    let deleted = client.delete_temporal_entity(&victim).await.expect("delete temporal entity failed");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete temporal entity: {:?}", deleted.body);

    let later_page = get_scoped(
        client,
        TEMPORAL_ENTITIES,
        &format!("type=Building&{WHOLE_HISTORY}&limit=100&offset=0"),
        Some(&map_id),
    )
    .await;
    assert_eq!(later_page.status, StatusCode::OK, "later temporal page: {:?}", later_page.body);
    let later_ids = ids_of(&later_page);
    assert!(
        !later_ids.contains(&victim),
        "a deleted temporal representation must not reach a later page (clause 4.3.6.7): {later_ids:?}"
    );

    let remaining = mapped_ids(client, &map_id).await;
    assert!(
        !remaining.contains(&victim),
        "clause 5.14.5 maps are pruned like any other: an entity no longer matching is removed: {remaining:?}"
    );
    assert_eq!(remaining.len(), 3, "only the deleted history leaves the map: {remaining:?}");

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.14: "If a subsequent request references an existent `EntityMap`, it
/// shall be used for the purposes of Entity registration matching, and queries
/// shall be filtered to only consider the Entities listed. [...] An `EntityMap`
/// fixes the Entities to be considered for subsequent requests based on it." A
/// follow-up that names an identifier the map does not list therefore considers
/// no entity at all, and must answer with nothing — never with the entities the
/// map does list, which is the widening an empty intersection read as "no
/// identifier filter" produces.
#[tokio::test]
async fn regression_entity_map_scoping_an_identifier_outside_the_map_returns_nothing() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let mapped: Vec<String> = (1..=2).map(|index| scope.id(&format!("Building:em-scope-{index}"))).collect();
    let outside = scope.id("Building:em-scope-outside");
    for id in mapped.iter().chain(once(&outside)) {
        create(
            client,
            &json!({
                "id": id,
                "type": "Building",
                "temperature": {"type": "Property", "value": 30.0},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
    }

    let filter = id_filter(&mapped.iter().map(String::as_str).collect::<Vec<_>>());
    let map_id = create_map(client, ENTITY_MAPS, &format!("type=Building&id={filter}")).await;
    assert_eq!(mapped_ids(client, &map_id).await.len(), 2, "the created map fixes both entities");

    let outside_filter = id_filter(&[outside.as_str()]);
    let unscoped = get_scoped(client, ENTITIES, &format!("type=Building&id={outside_filter}"), None).await;
    assert_eq!(
        ids_of(&unscoped),
        vec![outside.clone()],
        "without a map the entity is there to be found: {:?}",
        unscoped.body
    );

    let scoped = get_scoped(client, ENTITIES, &format!("type=Building&id={outside_filter}"), Some(&map_id)).await;
    assert_eq!(scoped.status, StatusCode::OK, "map-scoped query: {:?}", scoped.body);
    assert_eq!(
        ids_of(&scoped),
        Vec::<String>::new(),
        "an identifier the map does not list leaves nothing to consider (clause 5.5.14): {:?}",
        scoped.body
    );

    assert_eq!(
        mapped_ids(client, &map_id).await.len(),
        2,
        "clause 5.5.14 lets a follow-up name its own identifiers, so the entities it did not ask about stay in the map"
    );

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.3.6.7 empties a map whose entities all stopped matching, and clause
/// 5.5.14 keeps fixing what the requests that follow consider: an emptied map
/// considers no entity, so the next page must answer with nothing rather than
/// with every entity the remaining filters match.
#[tokio::test]
async fn regression_entity_map_scoping_an_emptied_map_returns_nothing() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let mapped: Vec<String> = (1..=2).map(|index| scope.id(&format!("Building:em-empty-{index}"))).collect();
    let decoy = scope.id("Building:em-empty-decoy");
    for id in mapped.iter().chain(once(&decoy)) {
        create(
            client,
            &json!({
                "id": id,
                "type": "Building",
                "temperature": {"type": "Property", "value": 30.0},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
    }

    let filter = id_filter(&mapped.iter().map(String::as_str).collect::<Vec<_>>());
    let map_id = create_map(client, ENTITY_MAPS, &format!("type=Building&id={filter}&q=temperature%3E20")).await;
    assert_eq!(mapped_ids(client, &map_id).await.len(), 2, "the created map fixes both matching entities");

    for id in &mapped {
        replace(
            client,
            id,
            &json!({
                "type": "Building",
                "temperature": {"type": "Property", "value": 1.0},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
    }

    let query = "type=Building&q=temperature%3E20&limit=100&offset=0";
    let emptying_page = get_scoped(client, ENTITIES, query, Some(&map_id)).await;
    assert_eq!(emptying_page.status, StatusCode::OK, "emptying page: {:?}", emptying_page.body);
    assert!(
        !ids_of(&emptying_page).iter().any(|id| mapped.contains(id)),
        "an entity that stopped matching does not reach the page (clause 4.3.6.7)"
    );
    assert_eq!(
        mapped_ids(client, &map_id).await,
        Vec::<String>::new(),
        "every entity of the map stopped matching, so the map is emptied (clause 4.3.6.7)"
    );

    let later_page = get_scoped(client, ENTITIES, query, Some(&map_id)).await;
    assert_eq!(later_page.status, StatusCode::OK, "later page: {:?}", later_page.body);
    let later_ids = ids_of(&later_page);
    assert!(
        !later_ids.contains(&decoy),
        "an emptied map fixes the result to no entity, so an entity it never listed must not appear (clause 5.5.14): {later_ids:?}"
    );
    assert!(
        !later_ids.iter().any(|id| id.starts_with(scope.id_prefix())),
        "an emptied map answers with nothing at all: {later_ids:?}"
    );

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.3.6.7 applies "filtering based on queries, geoqueries, scope queries
/// or attribute filters" only when a result page is rendered; for a retrieve
/// Entity operation "an Entity map can be used to make subsequent retrievals of
/// the same Entity more efficient", which applies no filter at all. A retrieval
/// under a map therefore checks nothing and sheds nothing, even when an entity
/// the map lists has since stopped matching the query that created it.
#[tokio::test]
async fn regression_entity_map_pruning_a_retrieval_under_a_map_sheds_nothing() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let ids: Vec<String> = (1..=2).map(|index| scope.id(&format!("Building:em-retrieve-{index}"))).collect();
    for id in &ids {
        create(
            client,
            &json!({
                "id": id,
                "type": "Building",
                "temperature": {"type": "Property", "value": 30.0},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
    }

    let filter = id_filter(&ids.iter().map(String::as_str).collect::<Vec<_>>());
    let map_id = create_map(client, ENTITY_MAPS, &format!("type=Building&id={filter}&q=temperature%3E20")).await;
    assert_eq!(mapped_ids(client, &map_id).await.len(), 2, "the created map fixes both matching entities");

    replace(
        client,
        &ids[1],
        &json!({
            "type": "Building",
            "temperature": {"type": "Property", "value": 1.0},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let retrieved = retrieve_scoped(client, &ids[0], &map_id).await;
    assert_eq!(retrieved.status, StatusCode::OK, "map-scoped retrieval: {:?}", retrieved.body);

    let remaining = mapped_ids(client, &map_id).await;
    assert_eq!(
        remaining.len(),
        2,
        "a retrieval applies none of the query's filters, so it checks nothing and sheds nothing (clause 4.3.6.7): {remaining:?}"
    );

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.14 holds a follow-up under a map to "the same parameters as in the
/// original request that created the `EntityMap`, except for the specification of
/// Entity identifiers", and clause 4.3.6.7 sheds only the entities "not matching
/// the filters at the time of checking". A follow-up that asks about one entity
/// of the map has checked nothing about the others, so they must survive in it.
#[tokio::test]
async fn regression_entity_map_pruning_a_narrower_follow_up_keeps_the_other_entities() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let ids: Vec<String> = (1..=3).map(|index| scope.id(&format!("Building:em-narrow-{index}"))).collect();
    for id in &ids {
        create(
            client,
            &json!({
                "id": id,
                "type": "Building",
                "temperature": {"type": "Property", "value": 30.0},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
    }

    let filter = id_filter(&ids.iter().map(String::as_str).collect::<Vec<_>>());
    let map_id = create_map(client, ENTITY_MAPS, &format!("type=Building&id={filter}&q=temperature%3E20")).await;
    assert_eq!(mapped_ids(client, &map_id).await.len(), 3, "the created map fixes all three entities");

    let one = id_filter(&[ids[0].as_str()]);
    let narrowed = get_scoped(client, ENTITIES, &format!("type=Building&q=temperature%3E20&id={one}"), Some(&map_id)).await;
    assert_eq!(narrowed.status, StatusCode::OK, "narrowed follow-up: {:?}", narrowed.body);
    assert_eq!(
        ids_of(&narrowed),
        vec![ids[0].clone()],
        "the follow-up answers with the one entity it asked for: {:?}",
        narrowed.body
    );

    let remaining = mapped_ids(client, &map_id).await;
    assert_eq!(
        remaining.len(),
        3,
        "the entities the follow-up did not ask about were never checked, so none of them leaves the map (clauses 5.5.14 and 4.3.6.7): {remaining:?}"
    );

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.3.6.7 removes from the map the "identifiers of Entities not matching
/// the filters at the time of checking" — of entities that were checked. An
/// entity held only by a context source that this page did not consult was not
/// checked at all, so nothing is known about it and it stays in the map, while
/// the entities this broker holds itself are checked on every page.
#[tokio::test]
async fn regression_entity_map_pruning_keeps_an_entity_whose_holder_was_not_consulted() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let configuration = MockServerConfig::from_environment().expect("valid mock server configuration");
    let source = ContextSourceServer::start(configuration).await.expect("context source must start");

    let local: Vec<String> = (1..=2).map(|index| scope.id(&format!("EmHolder:em-holder-local-{index}"))).collect();
    let remote = scope.id("EmHolder:em-holder-remote");
    for id in &local {
        create(
            client,
            &json!({
                "id": id,
                "type": "EmHolder",
                "temperature": {"type": "Property", "value": 30.0},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
    }

    let mut response_headers = HeaderMap::new();
    response_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"));
    let remote_body = json!([{
        "id": remote,
        "type": "EmHolder",
        "temperature": {"type": "Property", "value": 30.0},
        "@context": [TEST_SUITE_CONTEXT]
    }]);
    source
        .enqueue(
            Method::GET,
            CallbackPath::parse("/ngsi-ld/v1/entities").expect("absolute forwarding path"),
            MockResponse::builder()
                .status(StatusCode::OK)
                .headers(response_headers)
                .body(Bytes::from(serde_json::to_vec(&remote_body).expect("the mock response serializes")))
                .build(),
        )
        .await;

    let endpoint = source
        .endpoint(&CallbackPath::parse("/").expect("absolute root path"))
        .expect("the context source endpoint resolves")
        .to_string();
    let registration_id = scope.id("ContextSourceRegistration:em-holder");
    let created = client
        .create_registration(&json!({
            "id": registration_id,
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "EmHolder"}]}],
            "endpoint": endpoint,
            "operations": ["queryEntity"],
            "@context": [TEST_SUITE_CONTEXT]
        }))
        .await
        .expect("registration creation failed");
    assert_eq!(created.status, StatusCode::CREATED, "create registration: {:?}", created.body);

    let filter = id_filter(&[local[0].as_str(), local[1].as_str(), remote.as_str()]);
    let map_id = mint_map_through_query(client, &format!("type=EmHolder&id={filter}&q=temperature%3E20")).await;
    let mapped = mapped_ids(client, &map_id).await;
    assert!(
        mapped.contains(&remote),
        "the source that answered holds the remote entity in the map (clause 5.2.39): {mapped:?}"
    );
    assert_eq!(mapped.len(), 3, "the map fixes both local entities and the remote one: {mapped:?}");

    let deleted = client.delete_entity(&local[1]).await.expect("delete entity failed");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete entity: {:?}", deleted.body);

    // Clause 5.5.13 `local=true` contacts no context source, so the page that
    // follows checks what this broker holds and nothing of what the source does.
    let page = get_scoped(client, ENTITIES, "type=EmHolder&q=temperature%3E20&local=true&limit=100", Some(&map_id)).await;
    assert_eq!(page.status, StatusCode::OK, "local page: {:?}", page.body);

    let remaining = mapped_ids(client, &map_id).await;
    assert!(
        remaining.contains(&remote),
        "the holder of the remote entity was not consulted, so nothing was checked about it and it stays (clause 4.3.6.7): {remaining:?}"
    );
    assert!(
        !remaining.contains(&local[1]),
        "the deleted entity was checked here and no longer matches, so it leaves the map: {remaining:?}"
    );

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    client.delete_registration(&registration_id).await.expect("delete registration failed");
    source.shutdown().await.expect("context source shuts down");
    scope.finish().await.expect("cleanup failed");
}
