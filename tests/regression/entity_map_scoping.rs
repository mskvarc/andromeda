//! What an `EntityMap` fixes for the request that runs under it, in both
//! directions: a map may not widen a read, and it may not widen a purge.
//!
//! Clause 5.5.14: "If a subsequent request references an existent `EntityMap`,
//! it shall be used for the purposes of Entity registration matching, and
//! queries shall be filtered to only consider the Entities listed [...] An
//! `EntityMap` fixes the Entities to be considered for subsequent requests
//! based on it."
//!
//! An identifier list cannot express that on its own. An empty identifier list
//! reads as *no identifier constraint*, so a request narrowed to the empty
//! intersection of its own identifiers and a map's would answer with everything
//! the remaining filters match -- the exact opposite of what the map says. That
//! state is reachable through two documented routes: a map minted by a query
//! that matched nothing, and a map emptied by the clause 4.3.6.7 pruning. The
//! reads below put entities in the store that the same query would return
//! without a map, then assert that under an emptied map it returns none of
//! them.
//!
//! The same clause governs the purge. Table 6.4.3.3-2 makes `NGSILD-EntityMap`
//! a request header `DELETE /entities` shall support -- "If present, the
//! `EntityMap` supplied is used for determining the set of Entities requested
//! during the purge operation. The location of the `EntityMap` used in the purge
//! operation is returned in the response" -- so a purge under a map deletes the
//! entities the map lists and leaves every other entity its query matches
//! untouched, and answers with the map's location.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{header::jsonld_link_header, response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The current-entity query endpoint, which is also the purge endpoint.
const ENTITIES: &str = "/ngsi-ld/v1/entities";
/// The clause 6.23.3.1 POST binding of Query Entities.
const ENTITY_QUERY_POST: &str = "/ngsi-ld/v1/entityOperations/query";
/// The endpoint that creates an `EntityMap` over a current-entity query.
const ENTITY_MAPS: &str = "/ngsi-ld/v1/entityMaps";
/// The temporal query endpoint.
const TEMPORAL_ENTITIES: &str = "/ngsi-ld/v1/temporal/entities";
/// The endpoint that creates an `EntityMap` over a temporal query (clause 5.14.5).
const TEMPORAL_ENTITY_MAPS: &str = "/ngsi-ld/v1/temporal/entityMaps";
/// A temporal window wide enough to cover every recorded instance.
const WHOLE_HISTORY: &str = "timerel=before&timeAt=2099-01-01T00:00:00Z";
/// The response header carrying the location of the map an operation used.
const ENTITY_MAP_HEADER: &str = "NGSILD-EntityMap";

/// Send a `GET` under the test-suite context, optionally scoped to an `EntityMap`.
async fn get_scoped(client: &TestClient, path: &str, query: &str, map_id: Option<&str>) -> TestResponse {
    let mut request = client
        .raw(Method::GET, &format!("{path}?{query}"))
        .header("Accept", "application/ld+json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"));
    if let Some(map_id) = map_id {
        request = request.header(ENTITY_MAP_HEADER, map_id);
    }
    request.send().await.expect("map-scoped GET failed")
}

/// Send a clause 5.2.23 query body, optionally scoped to an `EntityMap`.
async fn post_scoped(client: &TestClient, path: &str, body: &Value, map_id: Option<&str>) -> TestResponse {
    let mut request = client
        .raw(Method::POST, path)
        .header("Accept", "application/ld+json")
        .header("Content-Type", "application/json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"));
    if let Some(map_id) = map_id {
        request = request.header(ENTITY_MAP_HEADER, map_id);
    }
    request.json(body).send().await.expect("map-scoped POST failed")
}

/// Run "Purge Entities" (clause 5.6.21) over `query`, optionally scoped to an
/// `EntityMap` through the Table 6.4.3.3-2 request header.
async fn purge_scoped(client: &TestClient, query: &str, map_id: Option<&str>) -> TestResponse {
    let mut request = client
        .raw(Method::DELETE, &format!("{ENTITIES}?{query}"))
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"));
    if let Some(map_id) = map_id {
        request = request.header(ENTITY_MAP_HEADER, map_id);
    }
    request.send().await.expect("map-scoped purge failed")
}

/// Create an entity and assert the broker accepted it.
///
/// Two attributes, because the clause 5.6.21.4 `drop` mode deletes the named
/// members of an entity rather than the entity, and a test of it needs one
/// member to go and one to stay.
async fn create(client: &TestClient, id: &str, entity_type: &str) {
    let body = json!({
        "id": id,
        "type": entity_type,
        "temperature": {"type": "Property", "value": 30.0},
        "name": {"type": "Property", "value": id},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = client.post_entity(&body).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED, "create entity: {:?}", response.body);
}

/// Whether each of `ids` still carries a `temperature` member, in the order given.
async fn carries_temperature(client: &TestClient, entity_type: &str, ids: &[&str]) -> Vec<bool> {
    let response = get_scoped(client, ENTITIES, &format!("type={entity_type}&id={}", id_filter(ids)), None).await;
    assert_eq!(response.status, StatusCode::OK, "attribute survey: {:?}", response.body);
    let body = response.json_value().expect("query body must be JSON");
    let entities = body.as_array().cloned().unwrap_or_default();
    ids.iter()
        .map(|id| {
            entities
                .iter()
                .find(|entity| entity.get("id").and_then(Value::as_str) == Some(id))
                .is_some_and(|entity| entity.get("temperature").is_some())
        })
        .collect()
}

/// Create a temporal entity carrying one observation and assert it was accepted.
async fn create_temporal(client: &TestClient, id: &str, entity_type: &str) {
    let body = json!({
        "id": id,
        "type": entity_type,
        "temperature": [{"type": "Property", "value": 30.0, "observedAt": "2024-01-01T00:00:00Z"}],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = client.create_temporal_entity(&body).await.expect("create temporal entity failed");
    assert_eq!(response.status, StatusCode::CREATED, "create temporal entity: {:?}", response.body);
}

/// Create an `EntityMap` over `query` and answer with its identifier.
async fn create_map(client: &TestClient, path: &str, query: &str) -> String {
    let response = get_scoped(client, path, query, None).await;
    assert_eq!(response.status, StatusCode::CREATED, "create EntityMap: {:?}", response.body);
    let body = response.json_value().expect("EntityMap body must be JSON");
    body["id"].as_str().expect("the created EntityMap must carry an id").to_owned()
}

/// Create an `EntityMap` over a clause 5.2.23 body and answer with its identifier.
///
/// A body-borne query is fingerprinted through its clause 5.2.33 selectors
/// (clause 5.5.14), so a follow-up on the POST binding can only reuse a map
/// created the same way.
async fn create_map_from_body(client: &TestClient, body: &Value) -> String {
    let response = post_scoped(client, ENTITY_MAPS, body, None).await;
    assert_eq!(response.status, StatusCode::CREATED, "create EntityMap from body: {:?}", response.body);
    let value = response.json_value().expect("EntityMap body must be JSON");
    value["id"].as_str().expect("the created EntityMap must carry an id").to_owned()
}

/// The identifiers a query answered with, sorted so a comparison does not
/// depend on the broker's result ordering.
fn ids_of(response: &TestResponse) -> Vec<String> {
    let mut ids: Vec<String> = response
        .json_value()
        .expect("query body must be JSON")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entity| entity.get("id").and_then(Value::as_str).map(String::from))
        .collect();
    ids.sort();
    ids
}

/// The identifiers an `EntityMap` resource lists (Table 5.2.39-2 `entityMap`).
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

/// The location of the map an operation answered with, if it answered with one.
fn used_map(response: &TestResponse) -> Option<String> {
    response
        .headers
        .get(ENTITY_MAP_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

/// The comma-joined, percent-encoded `id` filter that keeps a request to the
/// entities one isolated test owns.
fn id_filter(ids: &[&str]) -> String {
    ids.iter().map(|id| urlencoding::encode(id).into_owned()).collect::<Vec<_>>().join(",")
}

/// The identifiers still answered by an unmapped query over `ids`.
async fn surviving(client: &TestClient, entity_type: &str, ids: &[&str]) -> Vec<String> {
    let response = get_scoped(client, ENTITIES, &format!("type={entity_type}&id={}", id_filter(ids)), None).await;
    assert_eq!(response.status, StatusCode::OK, "survivor query: {:?}", response.body);
    ids_of(&response)
}

/// Three entities of one type, owned by the calling test.
async fn three_entities(scope: &TestEnvironment, entity_type: &str, local: &str) -> Vec<String> {
    let ids: Vec<String> = (1..=3).map(|index| scope.id(&format!("{entity_type}:{local}-{index}"))).collect();
    for id in &ids {
        create(scope.client(), id, entity_type).await;
    }
    ids
}

/// An identifier no entity carries, so a map created over it lists nothing.
fn absent_identifier(scope: &TestEnvironment) -> String {
    scope.id("Building:entity-map-scoping-absent")
}

/// Clause 5.5.14: "An `EntityMap` fixes the Entities to be considered for
/// subsequent requests based on it." A map that lists no entity at all fixes
/// the empty set, so a query under it answers with nothing -- never with
/// everything its remaining filters match, which is what an empty identifier
/// list would mean if it reached the store as "no identifier constraint".
#[tokio::test]
async fn regression_entity_map_scoping_an_emptied_map_returns_no_entity() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = three_entities(&scope, "Building", "em-scope-empty").await;
    let owned: Vec<&str> = ids.iter().map(String::as_str).collect();

    let unmapped = get_scoped(client, ENTITIES, &format!("type=Building&id={}", id_filter(&owned)), None).await;
    assert_eq!(unmapped.status, StatusCode::OK, "unmapped query: {:?}", unmapped.body);
    assert_eq!(ids_of(&unmapped).len(), 3, "the same query without a map returns every matching entity");

    let map_id = create_map(
        client,
        ENTITY_MAPS,
        &format!("type=Building&id={}", id_filter(&[absent_identifier(&scope).as_str()])),
    )
    .await;
    assert_eq!(mapped_ids(client, &map_id).await, Vec::<String>::new(), "the created map lists no entity");

    let mapped = get_scoped(client, ENTITIES, &format!("type=Building&id={}", id_filter(&owned)), Some(&map_id)).await;
    assert_eq!(mapped.status, StatusCode::OK, "map-scoped query: {:?}", mapped.body);
    assert_eq!(
        ids_of(&mapped),
        Vec::<String>::new(),
        "a map listing no entity fixes the empty set (clause 5.5.14), so the query considers nothing"
    );

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.14 on the temporal path: an emptied map fixes the empty set for
/// "Query Temporal Evolution of Entities" exactly as it does for the
/// current-state query, since clause 5.14.5 gives the temporal map the same
/// meaning.
#[tokio::test]
async fn regression_entity_map_scoping_an_emptied_map_returns_no_temporal_entity() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let ids: Vec<String> = (1..=3).map(|index| scope.id(&format!("Building:em-scope-temporal-empty-{index}"))).collect();
    for id in &ids {
        create_temporal(client, id, "Building").await;
    }
    let owned: Vec<&str> = ids.iter().map(String::as_str).collect();
    let filter = id_filter(&owned);

    let unmapped = get_scoped(client, TEMPORAL_ENTITIES, &format!("type=Building&id={filter}&{WHOLE_HISTORY}"), None).await;
    assert_eq!(unmapped.status, StatusCode::OK, "unmapped temporal query: {:?}", unmapped.body);
    assert_eq!(
        ids_of(&unmapped).len(),
        3,
        "the same temporal query without a map returns every matching history"
    );

    let absent = id_filter(&[absent_identifier(&scope).as_str()]);
    let map_id = create_map(client, TEMPORAL_ENTITY_MAPS, &format!("type=Building&id={absent}&{WHOLE_HISTORY}")).await;
    assert_eq!(
        mapped_ids(client, &map_id).await,
        Vec::<String>::new(),
        "the created temporal map lists no entity"
    );

    let mapped = get_scoped(client, TEMPORAL_ENTITIES, &format!("type=Building&id={filter}&{WHOLE_HISTORY}"), Some(&map_id)).await;
    assert_eq!(mapped.status, StatusCode::OK, "map-scoped temporal query: {:?}", mapped.body);
    assert_eq!(
        ids_of(&mapped),
        Vec::<String>::new(),
        "a temporal map listing no entity fixes the empty set (clauses 5.5.14 and 5.14.5)"
    );

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.14: "queries shall be filtered to only consider the Entities
/// listed". A map over a proper subset narrows a follow-up to exactly that
/// subset, whether the follow-up names its own identifiers or none at all.
#[tokio::test]
async fn regression_entity_map_scoping_a_partial_map_returns_only_its_entities() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = three_entities(&scope, "Building", "em-scope-subset").await;
    let owned: Vec<&str> = ids.iter().map(String::as_str).collect();
    let mapped_pair = [owned[0], owned[1]];

    let map_id = create_map(client, ENTITY_MAPS, &format!("type=Building&id={}", id_filter(&mapped_pair))).await;
    let mut expected: Vec<String> = mapped_pair.iter().map(ToString::to_string).collect();
    expected.sort();
    assert_eq!(mapped_ids(client, &map_id).await.len(), 2, "the created map fixes the two entities it selected");

    let wider = get_scoped(client, ENTITIES, &format!("type=Building&id={}", id_filter(&owned)), Some(&map_id)).await;
    assert_eq!(wider.status, StatusCode::OK, "map-scoped query naming all three: {:?}", wider.body);
    assert_eq!(
        ids_of(&wider),
        expected,
        "a follow-up may name its own identifiers but never widen past the map"
    );

    let unnamed = get_scoped(client, ENTITIES, "type=Building", Some(&map_id)).await;
    assert_eq!(unnamed.status, StatusCode::OK, "map-scoped query naming none: {:?}", unnamed.body);
    assert_eq!(ids_of(&unnamed), expected, "a follow-up naming no identifier is held to the whole map");

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clauses 5.5.14 and 5.14.5 on the temporal path: a map over a proper subset
/// narrows a temporal follow-up to exactly that subset.
#[tokio::test]
async fn regression_entity_map_scoping_a_partial_temporal_map_returns_only_its_entities() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let ids: Vec<String> = (1..=3).map(|index| scope.id(&format!("Building:em-scope-temporal-subset-{index}"))).collect();
    for id in &ids {
        create_temporal(client, id, "Building").await;
    }
    let owned: Vec<&str> = ids.iter().map(String::as_str).collect();
    let mapped_pair = [owned[0], owned[1]];
    let mut expected: Vec<String> = mapped_pair.iter().map(ToString::to_string).collect();
    expected.sort();

    let map_id = create_map(
        client,
        TEMPORAL_ENTITY_MAPS,
        &format!("type=Building&id={}&{WHOLE_HISTORY}", id_filter(&mapped_pair)),
    )
    .await;
    assert_eq!(
        mapped_ids(client, &map_id).await.len(),
        2,
        "the created temporal map fixes the two histories it selected"
    );

    let wider = get_scoped(
        client,
        TEMPORAL_ENTITIES,
        &format!("type=Building&id={}&{WHOLE_HISTORY}", id_filter(&owned)),
        Some(&map_id),
    )
    .await;
    assert_eq!(wider.status, StatusCode::OK, "map-scoped temporal query: {:?}", wider.body);
    assert_eq!(ids_of(&wider), expected, "a temporal follow-up never widens past the map");

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// The narrowing applies only to a request that named an empty set of
/// identifiers, never to one that named none: an unmapped query still answers
/// with everything it matches, and an unmapped purge still purges by query.
#[tokio::test]
async fn regression_entity_map_scoping_an_unmapped_query_returns_every_match() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = three_entities(&scope, "Building", "em-scope-unmapped").await;
    let owned: Vec<&str> = ids.iter().map(String::as_str).collect();
    let mut expected = ids.clone();
    expected.sort();

    let by_id = get_scoped(client, ENTITIES, &format!("type=Building&id={}", id_filter(&owned)), None).await;
    assert_eq!(by_id.status, StatusCode::OK, "unmapped query naming identifiers: {:?}", by_id.body);
    assert_eq!(ids_of(&by_id), expected, "a query that named identifiers and no map considers exactly them");

    let pattern = urlencoding::encode(&format!("^{}:Building:em-scope-unmapped-", scope.id_prefix())).into_owned();
    let by_pattern = get_scoped(client, ENTITIES, &format!("type=Building&idPattern={pattern}"), None).await;
    assert_eq!(by_pattern.status, StatusCode::OK, "unmapped query naming no identifier: {:?}", by_pattern.body);
    assert_eq!(
        ids_of(&by_pattern),
        expected,
        "a query that named no identifier at all places no identifier constraint"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.2.33 makes a clause 5.2.23 body's `entities[]` a union of selectors,
/// and compilation reads those selectors alone whenever the body carried one.
/// Clause 5.5.14 fixes the entities of *the request*, so the selectors have to
/// be narrowed too -- narrowing only the flat identifier list would leave the
/// POST binding answering with entities the map does not list.
#[tokio::test]
async fn regression_entity_map_scoping_a_body_selector_is_held_to_the_map() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = three_entities(&scope, "Building", "em-scope-body-subset").await;
    let mut expected = vec![ids[0].clone(), ids[1].clone()];
    expected.sort();

    let map_id = create_map_from_body(
        client,
        &json!({
            "type": "Query",
            "entities": [{"type": "Building", "id": [ids[0], ids[1]]}]
        }),
    )
    .await;
    assert_eq!(
        mapped_ids(client, &map_id).await.len(),
        2,
        "the created map fixes the two entities the body selected"
    );

    let mapped = post_scoped(
        client,
        ENTITY_QUERY_POST,
        &json!({
            "type": "Query",
            "entities": [{"type": "Building", "id": [ids[0], ids[1], ids[2]]}]
        }),
        Some(&map_id),
    )
    .await;
    assert_eq!(mapped.status, StatusCode::OK, "map-scoped POST query: {:?}", mapped.body);
    assert_eq!(ids_of(&mapped), expected, "a clause 5.2.33 selector is narrowed to the entities the map lists");

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// The emptied-map case on the POST binding: a union whose every selector was
/// narrowed to nothing admits nothing, so the body query answers with none of
/// the entities its type selector would otherwise match.
#[tokio::test]
async fn regression_entity_map_scoping_an_emptied_map_empties_a_body_selector() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = three_entities(&scope, "Building", "em-scope-body-empty").await;

    let unmapped = post_scoped(
        client,
        ENTITY_QUERY_POST,
        &json!({
            "type": "Query",
            "entities": [{"type": "Building", "id": [ids[0], ids[1], ids[2]]}]
        }),
        None,
    )
    .await;
    assert_eq!(unmapped.status, StatusCode::OK, "unmapped POST query: {:?}", unmapped.body);
    assert_eq!(ids_of(&unmapped).len(), 3, "the same body query without a map returns every matching entity");

    let map_id = create_map_from_body(
        client,
        &json!({
            "type": "Query",
            "entities": [{"type": "Building", "id": [absent_identifier(&scope)]}]
        }),
    )
    .await;
    assert_eq!(mapped_ids(client, &map_id).await, Vec::<String>::new(), "the created map lists no entity");

    let mapped = post_scoped(
        client,
        ENTITY_QUERY_POST,
        &json!({
            "type": "Query",
            "entities": [{"type": "Building", "id": [ids[0], ids[1], ids[2]]}]
        }),
        Some(&map_id),
    )
    .await;
    assert_eq!(mapped.status, StatusCode::OK, "map-scoped POST query: {:?}", mapped.body);
    assert_eq!(
        ids_of(&mapped),
        Vec::<String>::new(),
        "an emptied map leaves the union of selectors admitting nothing (clause 5.5.14)"
    );

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// Table 6.4.3.3-2: "If present, the `EntityMap` supplied is used for determining
/// the set of Entities requested during the purge operation." The purge deletes
/// the entities the map lists and leaves the ones its query matched but the map
/// does not list.
#[tokio::test]
async fn regression_purge_entity_map_deletes_only_the_mapped_entities() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = three_entities(&scope, "Building", "em-purge-subset").await;
    let owned: Vec<&str> = ids.iter().map(String::as_str).collect();
    let mapped_pair = [owned[0], owned[1]];

    let map_id = create_map(client, ENTITY_MAPS, &format!("type=Building&id={}", id_filter(&mapped_pair))).await;
    assert_eq!(mapped_ids(client, &map_id).await.len(), 2, "the created map fixes the two entities it selected");

    let purged = purge_scoped(client, &format!("type=Building&id={}", id_filter(&owned)), Some(&map_id)).await;
    assert_eq!(purged.status, StatusCode::NO_CONTENT, "map-scoped purge: {:?}", purged.body);

    assert_eq!(
        surviving(client, "Building", &owned).await,
        vec![ids[2].clone()],
        "only the entities the map listed are purged (Table 6.4.3.3-2)"
    );

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// Table 6.4.3.3-2: "The location of the `EntityMap` used in the purge operation
/// is returned in the response."
#[tokio::test]
async fn regression_purge_entity_map_returns_the_map_location() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = three_entities(&scope, "Building", "em-purge-location").await;
    let owned: Vec<&str> = ids.iter().map(String::as_str).collect();

    let map_id = create_map(client, ENTITY_MAPS, &format!("type=Building&id={}", id_filter(&owned[..1]))).await;

    let purged = purge_scoped(client, &format!("type=Building&id={}", id_filter(&owned)), Some(&map_id)).await;
    assert_eq!(purged.status, StatusCode::NO_CONTENT, "map-scoped purge: {:?}", purged.body);
    assert_eq!(
        used_map(&purged).as_deref(),
        Some(map_id.as_str()),
        "the purge answers with the location of the map it used"
    );

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// A purge that supplies no map is unchanged: clause 5.6.21.4 has it run its
/// own Query Entities and delete everything that query matched, and it answers
/// with no map location because it used no map.
#[tokio::test]
async fn regression_purge_entity_map_without_the_header_purges_by_query() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = three_entities(&scope, "Building", "em-purge-unmapped").await;
    let owned: Vec<&str> = ids.iter().map(String::as_str).collect();

    let purged = purge_scoped(client, &format!("type=Building&id={}", id_filter(&owned)), None).await;
    assert_eq!(purged.status, StatusCode::NO_CONTENT, "unmapped purge: {:?}", purged.body);
    assert_eq!(used_map(&purged), None, "a purge that used no map has no location to return");
    assert_eq!(
        surviving(client, "Building", &owned).await,
        Vec::<String>::new(),
        "an unmapped purge deletes everything its query matched (clause 5.6.21.4)"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.14: "An `EntityMap` fixes the Entities to be considered." A purge
/// under a map that lists no entity deletes nothing at all -- the destructive
/// mirror of the emptied-map read, where reading "no identifier constraint" out
/// of the empty intersection would delete every entity the query matched.
#[tokio::test]
async fn regression_purge_entity_map_an_emptied_map_deletes_nothing() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = three_entities(&scope, "Building", "em-purge-empty").await;
    let owned: Vec<&str> = ids.iter().map(String::as_str).collect();
    let mut expected = ids.clone();
    expected.sort();

    let map_id = create_map(
        client,
        ENTITY_MAPS,
        &format!("type=Building&id={}", id_filter(&[absent_identifier(&scope).as_str()])),
    )
    .await;
    assert_eq!(mapped_ids(client, &map_id).await, Vec::<String>::new(), "the created map lists no entity");

    let purged = purge_scoped(client, &format!("type=Building&id={}", id_filter(&owned)), Some(&map_id)).await;
    assert_eq!(purged.status, StatusCode::NO_CONTENT, "purge under an emptied map: {:?}", purged.body);
    assert_eq!(used_map(&purged).as_deref(), Some(map_id.as_str()), "the purge answers with the map it used");
    assert_eq!(
        surviving(client, "Building", &owned).await,
        expected,
        "a map listing no entity fixes the empty set, so the purge deletes nothing"
    );

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.14: "Subsequent requests referencing an `EntityMap` shall use the
/// same parameters as in the original request that created the `EntityMap`,
/// except for the specification of Entity identifiers or parameters related to
/// pagination." A purge that changed a filter is refused with the 400 Table
/// 6.4.3.3-3 admits, and deletes nothing.
#[tokio::test]
async fn regression_purge_entity_map_a_changed_filter_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = three_entities(&scope, "Building", "em-purge-changed").await;
    let owned: Vec<&str> = ids.iter().map(String::as_str).collect();
    let mut expected = ids.clone();
    expected.sort();

    let map_id = create_map(client, ENTITY_MAPS, &format!("type=Building&id={}", id_filter(&owned))).await;

    let refused = purge_scoped(client, &format!("type=Building&q=temperature%3E1&id={}", id_filter(&owned)), Some(&map_id)).await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST, "purge under a changed request: {:?}", refused.body);
    assert_eq!(surviving(client, "Building", &owned).await, expected, "a refused purge deletes nothing");

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.14: "If an `EntityMap` has expired, or cannot be accessed, no
/// inference can be made as to which entities are held within the Context
/// Sources." A purge cannot create a replacement -- clause 5.5.14 does not list
/// Purge Entities among the operations of a distributed transactional request,
/// Table 6.4.3.3-1 gives it no `entityMap` parameter to ask for one with, and
/// Table 6.4.3.3-3 admits no 201 to report one through -- so it runs on its own
/// query and answers with no map location.
#[tokio::test]
async fn regression_purge_entity_map_an_unknown_map_purges_by_query() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = three_entities(&scope, "Building", "em-purge-unknown").await;
    let owned: Vec<&str> = ids.iter().map(String::as_str).collect();
    let unknown = scope.id("EntityMap:em-purge-never-created");

    let purged = purge_scoped(client, &format!("type=Building&id={}", id_filter(&owned)), Some(&unknown)).await;
    assert_eq!(purged.status, StatusCode::NO_CONTENT, "purge under an unknown map: {:?}", purged.body);
    assert_eq!(used_map(&purged), None, "no map was used, so there is no location to return");
    assert_eq!(
        surviving(client, "Building", &owned).await,
        Vec::<String>::new(),
        "an unresolvable reference yields no inference, so the purge runs on its own query"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.14 narrows a follow-up to the intersection, not to the map: a
/// request that names a subset of the mapped entities gets that subset, so the
/// map bounds what a follow-up may ask for without dictating it.
#[tokio::test]
async fn regression_entity_map_scoping_a_follow_up_may_narrow_within_the_map() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = three_entities(&scope, "Building", "em-scope-narrow").await;
    let owned: Vec<&str> = ids.iter().map(String::as_str).collect();

    let map_id = create_map(client, ENTITY_MAPS, &format!("type=Building&id={}", id_filter(&owned))).await;
    assert_eq!(mapped_ids(client, &map_id).await.len(), 3, "the created map fixes all three entities");

    let narrowed = get_scoped(client, ENTITIES, &format!("type=Building&id={}", id_filter(&owned[..1])), Some(&map_id)).await;
    assert_eq!(narrowed.status, StatusCode::OK, "map-scoped query naming one: {:?}", narrowed.body);
    assert_eq!(
        ids_of(&narrowed),
        vec![ids[0].clone()],
        "clause 5.5.14 lets a follow-up specify its own identifiers within the map"
    );

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// The same intersection on the destructive side: a purge under a map that
/// lists more than the purge asks for deletes only what both name.
#[tokio::test]
async fn regression_purge_entity_map_a_purge_may_narrow_within_the_map() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = three_entities(&scope, "Building", "em-purge-narrow").await;
    let owned: Vec<&str> = ids.iter().map(String::as_str).collect();

    let map_id = create_map(client, ENTITY_MAPS, &format!("type=Building&id={}", id_filter(&owned))).await;
    assert_eq!(mapped_ids(client, &map_id).await.len(), 3, "the created map fixes all three entities");

    let purged = purge_scoped(client, &format!("type=Building&id={}", id_filter(&owned[..1])), Some(&map_id)).await;
    assert_eq!(purged.status, StatusCode::NO_CONTENT, "map-scoped purge naming one: {:?}", purged.body);

    let mut expected = vec![ids[1].clone(), ids[2].clone()];
    expected.sort();
    assert_eq!(
        surviving(client, "Building", &owned).await,
        expected,
        "only the entity both the purge and the map named is deleted"
    );

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.21.4: "when a restrictive list of Entity member names is present,
/// the implementation shall delete the given set of Attributes with those member
/// names from all Entities that can be found locally using retrieved list of
/// Entity ids." The `drop` mode reads the same retrieved list, so Table
/// 6.4.3.3-2's map bounds which entities lose the named member.
#[tokio::test]
async fn regression_purge_entity_map_drop_mode_is_held_to_the_map() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids = three_entities(&scope, "Building", "em-purge-drop").await;
    let owned: Vec<&str> = ids.iter().map(String::as_str).collect();

    let map_id = create_map(client, ENTITY_MAPS, &format!("type=Building&id={}", id_filter(&owned[..2]))).await;
    assert_eq!(mapped_ids(client, &map_id).await.len(), 2, "the created map fixes the two entities it selected");

    let purged = purge_scoped(client, &format!("type=Building&id={}&drop=temperature", id_filter(&owned)), Some(&map_id)).await;
    assert_eq!(purged.status, StatusCode::NO_CONTENT, "map-scoped drop purge: {:?}", purged.body);
    assert_eq!(used_map(&purged).as_deref(), Some(map_id.as_str()), "the purge answers with the map it used");

    let mut still_there = surviving(client, "Building", &owned).await;
    still_there.sort();
    let mut expected = ids.clone();
    expected.sort();
    assert_eq!(still_there, expected, "the drop mode deletes members, never entities");
    assert_eq!(
        carries_temperature(client, "Building", &owned).await,
        vec![false, false, true],
        "only the entities the map listed lose the dropped member"
    );

    client.delete_entity_map(&map_id).await.expect("delete EntityMap failed");
    scope.finish().await.expect("cleanup failed");
}
