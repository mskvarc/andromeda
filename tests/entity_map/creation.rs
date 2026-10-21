//! Creating an `EntityMap` from a query, over both bindings of
//! `/entityMaps` (NGSI-LD 5.2.32, 6.3.19).

use crate::{
    BAD_REQUEST_DATA_TYPE,
    BASE_URL,
    INVALID_REQUEST_TYPE,
    LD_CONTEXT_NOT_AVAILABLE_TYPE,
    cleanup_entity_map,
    client,
    create_entity,
    create_entity_map_from_get,
    create_entity_map_from_post,
    delete_entity,
    get_entity_map,
    jsonld_link_value,
    setup_single_vehicle,
    vehicle_entity_with_speed,
};
use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use chrono::Utc;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

#[tokio::test]
async fn entity_map_create_from_get_by_type_returns_201_with_headers() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let id1 = scope.id("Vehicle:em-create1");
    let id2 = scope.id("Vehicle:em-create2");

    setup_single_vehicle(&c, &id1).await;
    setup_single_vehicle(&c, &id2).await;

    let (status, headers, body) = create_entity_map_from_get(&c, &scope, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let map_id = body["id"].as_str().expect("map id missing").to_owned();
    pretty_assertions::assert_eq!(body["type"], "EntityMap");
    assert!(body["expiresAt"].is_string());

    let entity_map = body["entityMap"].as_object().expect("entityMap should be object");
    pretty_assertions::assert_eq!(entity_map.len(), 2);
    assert!(entity_map.contains_key(&id1));
    assert!(entity_map.contains_key(&id2));

    let location = headers
        .get("location")
        .expect("location header should be present")
        .to_str()
        .expect("location must be ASCII");
    assert!(location.contains("entityMaps/"));
    assert!(headers.get("NGSILD-EntityMap").is_some());

    let (status, _, _) = get_entity_map(&c, &map_id, "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id1).await;
    delete_entity(&c, &id2).await;
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_create_from_get_with_q_filter_returns_only_matching_ids() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let id1 = scope.id("Vehicle:em-q1");
    let id2 = scope.id("Vehicle:em-q2");

    let (status, _) = create_entity(&c, &vehicle_entity_with_speed(&id1, 100)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let (status, _) = create_entity(&c, &vehicle_entity_with_speed(&id2, 30)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = create_entity_map_from_get(&c, &scope, "type=Vehicle&q=speed%3E50").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let map_id = body["id"].as_str().expect("map id missing").to_owned();
    let entity_map = body["entityMap"].as_object().expect("entityMap should be object");
    pretty_assertions::assert_eq!(entity_map.len(), 1);
    assert!(entity_map.contains_key(&id1));

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id1).await;
    delete_entity(&c, &id2).await;
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_create_from_get_with_lifetime_pt5m_sets_expires_at_in_range() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:em-lt");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, &scope, "type=Vehicle&entityMapLifetime=PT5M").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let map_id = body["id"].as_str().expect("map id missing").to_owned();
    let expires_at = body["expiresAt"].as_str().expect("expiresAt should exist");
    let parsed = expires_at.parse::<NgsiLdDateTime>().expect("expiresAt should be RFC3339").get();
    let diff = parsed.signed_duration_since(Utc::now());
    assert!(diff.num_seconds() > 240 && diff.num_seconds() <= 300);

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_create_from_get_without_a_narrowing_input_returns_400() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();

    // Clause 5.14.4.4 restates the clause 5.7.2.4 rule word for word: the
    // narrowing inputs are the type selector, the attribute list, the query,
    // the geoquery and the local scope, and an identifier is none of them.
    let (status, _, body) = create_entity_map_from_get(&c, &scope, "id=urn:ngsi-ld:Vehicle:em-too-wide").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());

    // Input (e) admits a creation limited to this broker.
    let (status, _, _) = create_entity_map_from_get(&c, &scope, "local=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_create_from_get_with_invalid_lifetime_returns_400() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:em-lt-invalid");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, &scope, "type=Vehicle&entityMapLifetime=NOT_A_DURATION").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());

    delete_entity(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_create_from_post_with_entities_filter_returns_201() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:em-post");
    setup_single_vehicle(&c, &id).await;

    let body = json!({
        "type": "Query",
        "entities": [{ "type": "Vehicle" }]
    });
    let (status, _, body) = create_entity_map_from_post(&c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{body}");

    let map_id = body["id"].as_str().expect("map id missing").to_owned();
    pretty_assertions::assert_eq!(body["type"], "EntityMap");

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_create_from_post_without_filters_returns_400() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();

    let body = json!({ "type": "Query" });
    let (status, _, body) = create_entity_map_from_post(&c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_create_from_get_with_empty_result_returns_201_empty_entity_map() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();

    let (status, _, body) = create_entity_map_from_get(&c, &scope, "type=NonExistentType12345").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let map_id = body["id"].as_str().expect("map id missing").to_owned();
    let entity_map = body["entityMap"].as_object().expect("entityMap should be object");
    assert!(entity_map.is_empty());

    cleanup_entity_map(&c, &map_id).await;
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_create_from_get_with_many_entities_returns_full_entity_map() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let mut ids = Vec::new();

    for index in 0..50 {
        let id = scope.id(&format!("Vehicle:em-many-{index}"));
        setup_single_vehicle(&c, &id).await;
        ids.push(id);
    }

    let (status, _, body) = create_entity_map_from_get(&c, &scope, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let map_id = body["id"].as_str().expect("map id missing").to_owned();
    let entity_map = body["entityMap"].as_object().expect("entityMap should be object");
    pretty_assertions::assert_eq!(entity_map.len(), 50);

    cleanup_entity_map(&c, &map_id).await;
    for id in ids {
        delete_entity(&c, &id).await;
    }
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn entity_map_api_rejects_unknown_query_parameter_with_400() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();

    let resp = c
        .request(Method::GET, format!("{BASE_URL}/entityMaps?type=Vehicle&invalidParam=invalidValue"))
        .header("Link", jsonld_link_value())
        .send()
        .await
        .expect("request failed");

    pretty_assertions::assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: Value = resp.json().await.unwrap_or_default();
    pretty_assertions::assert_eq!(body["type"], INVALID_REQUEST_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

/// The baseline every creation case rests on: an entity provisioned with the
/// Core Context is accepted, so a map built over it maps a real entity.
#[tokio::test]
async fn entity_map_create_entity_using_core_context_succeeds_for_baseline() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:em-baseline");

    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "@context": CORE_CONTEXT
    });

    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    delete_entity(&c, &id).await;
    scope.finish().await.expect("cleanup failed");
}

/// A remote `@context` at an address nothing answers on, so its resolution
/// fails with a connection refusal rather than a slow timeout (clause 5.5.6).
const UNREACHABLE_CONTEXT_LINK: &str = r#"<http://127.0.0.1:9/unreachable.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#;

/// POST `query` to `path` as `application/ld+json` under an inline `@context`
/// declaring a JSON-LD Scoped Context, and as `application/json` under an
/// unreachable context `Link`, answering each status and problem type and
/// whether a map was created.
async fn refused_creations(c: &reqwest::Client, path: &str) -> [(StatusCode, Value, bool); 2] {
    let query = json!({"type": "Query", "entities": [{"type": "Vehicle"}]});
    let mut scoped = query.clone(); // clone: the unreachable case posts the query without the context
    scoped["@context"] = json!([{"speed": {"@id": "https://example.org/speed", "@context": {}}}, CORE_CONTEXT.as_str()]);
    let scoped = c
        .post(format!("{BASE_URL}{path}"))
        .header("Content-Type", "application/ld+json")
        .json(&scoped)
        .send()
        .await
        .expect("scoped request");
    let unreachable = c
        .post(format!("{BASE_URL}{path}"))
        .header("Content-Type", "application/json")
        .header("Link", UNREACHABLE_CONTEXT_LINK)
        .json(&query)
        .send()
        .await
        .expect("unreachable request");
    [answered(scoped).await, answered(unreachable).await]
}

/// The status, problem type and whether a map was created, of one answer.
async fn answered(response: reqwest::Response) -> (StatusCode, Value, bool) {
    let status = response.status();
    let created = response.headers().contains_key("location") || response.headers().contains_key("ngsild-entitymap");
    let body: Value = response.json().await.unwrap_or_default();
    (status, body["type"].clone(), created) // clone: the type outlives the body it was read from
}

/// Clauses 5.5.6 and 5.5.7: an Entity Map creation from a posted query admits
/// its user `@context` first -- a scoped one is `BadRequestData`, an
/// unreachable one `LdContextNotAvailable` -- and creates no map.
#[tokio::test]
async fn entity_map_create_from_post_refuses_a_scoped_or_unreachable_context() {
    let scope = TestEnvironment::from_suite("entity_map").await.expect("isolated test environment");
    let c = client();
    for path in ["/entityMaps", "/temporal/entityMaps"] {
        let [scoped, unreachable] = refused_creations(&c, path).await;
        pretty_assertions::assert_eq!(scoped, (StatusCode::BAD_REQUEST, json!(BAD_REQUEST_DATA_TYPE.as_str()), false), "{path}");
        pretty_assertions::assert_eq!(
            unreachable,
            (StatusCode::GATEWAY_TIMEOUT, json!(LD_CONTEXT_NOT_AVAILABLE_TYPE.as_str()), false),
            "{path}"
        );
    }
    scope.finish().await.expect("cleanup failed");
}
