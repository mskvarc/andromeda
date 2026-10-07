//! References of any length, and identifiers held to their bound, end to end.
//!
//! Clause 4.6.2 sets no length on a URI, and a Relationship's `object` (Table 5.2.6-1, "Valid URI or an Array of Valid URIs") and every member of a `ListRelationship`'s `objectList` (Table 5.2.37-1) is typed as nothing more, so a reference is bounded only by the request-body limit. An identifier, the Entity `id` among them, keeps its documented 1024-octet bound because it is an index key.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{problem_details::validate_problem_details, response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::{context::TEST_SUITE_CONTEXT, problem_detail::BAD_REQUEST_DATA};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;

/// The entity type every fixture in this module carries.
const FIXTURE_TYPE: &str = "LongReference";

/// The identifier bound, in octets, that every Entity id is held to.
const IDENTIFIER_BOUND: usize = 1024;

/// A URN of exactly `length` octets whose tail varies with `seed`, so two references of one length are distinct and neither compresses to nothing.
fn reference_of_length(prefix: &str, seed: u64, length: usize) -> String {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let tail: String = (prefix.len()..length)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            char::from(b"abcdefghijklmnopqrstuvwxyz0123456789"[usize::try_from(state % 36).expect("a small index")])
        })
        .collect();
    let reference = format!("{prefix}{tail}");
    assert_eq!(reference.len(), length, "the fixture has the requested width");
    reference
}

async fn create(client: &TestClient, entity: &Value) {
    let response = client.post_entity(entity).send().await.expect("create entity request failed");
    assert_eq!(response.status, StatusCode::CREATED, "create entity: {:?}", response.body);
}

/// The identifiers of the entities one query answered with, in response order.
async fn queried_ids(client: &TestClient, query: &str) -> Vec<String> {
    let response = client
        .query_entities(query, "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query entities request failed");
    assert_eq!(response.status, StatusCode::OK, "query {query}: {:?}", response.body);
    let mut ids: Vec<String> = response
        .json_value()
        .expect("query response must be JSON")
        .as_array()
        .expect("query response must be an array")
        .iter()
        .filter_map(|entity| entity["id"].as_str().map(str::to_owned))
        .collect();
    ids.sort();
    ids
}

/// A `q` filter over this test's own entities.
fn filter(ids: &[&str], q: &str) -> String {
    format!(
        "type={FIXTURE_TYPE}&local=true&id={}&q={}",
        ids.iter().map(|id| urlencoding::encode(id).into_owned()).collect::<Vec<_>>().join(","),
        urlencoding::encode(q)
    )
}

/// The normalized `objectList` of clause 4.5.22.2 naming `members`, in order:
/// each element a JSON object whose single `object` member holds the URI.
fn normalized_object_list<T: AsRef<str>>(members: &[T]) -> Value {
    Value::Array(members.iter().map(|member| json!({"object": member.as_ref()})).collect())
}

/// Retrieve one entity in its normalized representation.
async fn retrieve(client: &TestClient, id: &str) -> Value {
    let response = client
        .get_entity(id, "", "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve entity request failed");
    assert_eq!(response.status, StatusCode::OK, "retrieve {id}: {:?}", response.body);
    response.json_value().expect("an entity must be JSON")
}

/// The `(value_uri, value_long_uri)` pair the current-state projection holds
/// for the default instance of one attribute.
async fn uri_placement(pool: &PgPool, entity_id: &str, local_name: &str) -> (Option<String>, Option<String>) {
    sqlx::query_as(
        "SELECT c.value_uri, c.value_long_uri FROM current_attribute_values AS c \
         JOIN entities AS e ON e.entity_key = c.entity_key \
         JOIN attribute_names AS an ON an.attribute_name_key = c.attribute_name_key \
         WHERE e.id = $1 AND c.dataset_id IS NULL AND an.name LIKE '%' || $2",
    )
    .bind(entity_id)
    .bind(local_name)
    .fetch_one(pool)
    .await
    .expect("the default instance of the Relationship must be projected")
}

/// Assert that `response` refuses the request as `BadRequestData`.
fn assert_bad_request_data(response: &TestResponse, operation: &str) -> Value {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{operation}: {:?}", response.body);
    let problem = response.json_value().expect("problem details must be JSON");
    let verdict = validate_problem_details(&problem, BAD_REQUEST_DATA);
    assert!(verdict.is_ok(), "{operation} must be refused as BadRequestData: {verdict:?} {problem}");
    problem
}

/// Clause 4.6.2 and Table 5.2.37-1: a 6000-octet `objectList` member is a URI. The Entity is created, its representation returns the member byte for byte, and a clause 4.9 equality on the `ListRelationship` finds it.
#[tokio::test]
async fn regression_long_uri_references_a_list_relationship_member_of_six_thousand_octets_round_trips() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id(&format!("{FIXTURE_TYPE}:list"));
    let long = reference_of_length("urn:ngsi-ld:StopTimeUpdate:", 1, 6000);
    let short = "urn:ngsi-ld:StopTimeUpdate:short".to_owned();

    create(
        client,
        &json!({
            "id": id,
            "type": FIXTURE_TYPE,
            "servedBy": {"type": "ListRelationship", "objectList": [short, long]},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let entity = retrieve(client, &id).await;
    assert_eq!(
        entity["servedBy"]["objectList"],
        normalized_object_list(&[&short, &long]),
        "every member must round-trip byte for byte"
    );

    assert_eq!(
        queried_ids(client, &filter(&[&id], &format!("servedBy==\"{long}\""))).await,
        vec![id.clone()],
        "an equality on the long member must select the entity"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.6-1: a single Relationship `object` of 6000 octets is a URI. It is projected into the overflow column, a clause 4.9 equality selects it and an inequality excludes it, and a traversal through it matches nothing, without an error, because no local entity can be named by it.
#[tokio::test]
async fn regression_long_uri_references_a_single_relationship_object_of_six_thousand_octets_is_compared_and_not_traversed() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let long_source = scope.id(&format!("{FIXTURE_TYPE}:long-source"));
    let short_source = scope.id(&format!("{FIXTURE_TYPE}:short-source"));
    let target = scope.id(&format!("{FIXTURE_TYPE}:target"));
    let long = reference_of_length("urn:ngsi-ld:Stop:", 2, 6000);

    create(
        client,
        &json!({"id": target, "type": FIXTURE_TYPE, "name": {"type": "Property", "value": "x"}, "@context": [TEST_SUITE_CONTEXT]}),
    )
    .await;
    create(
        client,
        &json!({"id": long_source, "type": FIXTURE_TYPE, "link": {"type": "Relationship", "object": long}, "@context": [TEST_SUITE_CONTEXT]}),
    )
    .await;
    create(
        client,
        &json!({"id": short_source, "type": FIXTURE_TYPE, "link": {"type": "Relationship", "object": target}, "@context": [TEST_SUITE_CONTEXT]}),
    )
    .await;

    assert_eq!(
        retrieve(client, &long_source).await["link"]["object"],
        json!(long),
        "the object must round-trip byte for byte"
    );
    assert_eq!(
        uri_placement(scope.database_pool(), &long_source, "link").await,
        (None, Some(long.clone())),
        "an object past the B-tree bound is projected into the overflow column"
    );
    assert_eq!(
        uri_placement(scope.database_pool(), &short_source, "link").await,
        (Some(target.clone()), None),
        "an object naming a local entity is projected into the traversed column"
    );

    let sources = [long_source.as_str(), short_source.as_str()];
    assert_eq!(
        queried_ids(client, &filter(&sources, &format!("link==\"{long}\""))).await,
        vec![long_source.clone()],
        "an equality on the long object must select its source"
    );
    assert_eq!(
        queried_ids(client, &filter(&sources, &format!("link!=\"{long}\""))).await,
        vec![short_source.clone()],
        "an inequality on the long object must exclude its source"
    );
    assert_eq!(
        queried_ids(client, &format!("{}&join=inline&joinLevel=1", filter(&sources, "link{name}==\"x\""))).await,
        vec![short_source.clone()],
        "a traversal reaches the local target and nothing through the long object"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.8: a batch upsert carrying long references succeeds for every entity, rather than failing the batch on a reference's length.
#[tokio::test]
async fn regression_long_uri_references_a_batch_upsert_carrying_long_references_succeeds() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let first = scope.id(&format!("{FIXTURE_TYPE}:batch-1"));
    let second = scope.id(&format!("{FIXTURE_TYPE}:batch-2"));
    let long_object = reference_of_length("urn:ngsi-ld:Stop:", 3, 6000);
    let long_member = reference_of_length("urn:ngsi-ld:StopTimeUpdate:", 4, 4096);

    let batch = json!([
        {"id": first, "type": FIXTURE_TYPE, "link": {"type": "Relationship", "object": long_object}, "@context": [TEST_SUITE_CONTEXT]},
        {"id": second, "type": FIXTURE_TYPE, "servedBy": {"type": "ListRelationship", "objectList": [long_member]}, "@context": [TEST_SUITE_CONTEXT]}
    ]);
    let response = client.batch_upsert(&batch).await.expect("batch upsert request failed");
    assert!(
        response.status == StatusCode::CREATED || response.status == StatusCode::NO_CONTENT,
        "every entity of the batch must be upserted: {} {:?}",
        response.status,
        response.body
    );

    assert_eq!(retrieve(client, &first).await["link"]["object"], json!(long_object));
    assert_eq!(
        retrieve(client, &second).await["servedBy"]["objectList"],
        normalized_object_list(&[&long_member])
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.11: the temporal evolution of a Relationship records its object as given, however long, and returns it.
#[tokio::test]
async fn regression_long_uri_references_temporal_history_records_and_returns_a_long_object() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id(&format!("{FIXTURE_TYPE}:history"));
    let first = reference_of_length("urn:ngsi-ld:Stop:", 5, 6000);
    let second = reference_of_length("urn:ngsi-ld:Stop:", 6, 2048);

    let created = client
        .create_temporal_entity(&json!({
            "id": id,
            "type": FIXTURE_TYPE,
            "link": [
                {"type": "Relationship", "object": first, "observedAt": "2026-01-01T00:00:00Z"},
                {"type": "Relationship", "object": second, "observedAt": "2026-01-01T00:01:00Z"}
            ],
            "@context": [TEST_SUITE_CONTEXT]
        }))
        .await
        .expect("create temporal entity request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create temporal entity: {:?}", created.body);

    let history = client
        .get_temporal_entity(&id, "", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve temporal entity request failed");
    assert_eq!(history.status, StatusCode::OK, "retrieve temporal entity: {:?}", history.body);
    let body = history.json_value().expect("a temporal entity must be JSON");
    let mut objects: Vec<&str> = body["link"]
        .as_array()
        .expect("the Relationship's instances")
        .iter()
        .filter_map(|instance| instance["object"].as_str())
        .collect();
    objects.sort_unstable();
    let mut expected = vec![first.as_str(), second.as_str()];
    expected.sort_unstable();
    assert_eq!(objects, expected, "every recorded object must be returned byte for byte");

    scope.finish().await.expect("cleanup failed");
}

/// An Entity id is an index key and keeps its documented bound: one octet past it is refused as `BadRequestData`, whose detail names the member and the bound, and nothing is stored. A batch reports it against the entity rather than failing the request: Table 5.2.17-1 types `entityId` as any valid URI, so the over-long `id` is what the error names (clause 5.6.8.4).
#[tokio::test]
async fn regression_long_uri_references_an_entity_id_one_octet_past_the_bound_is_refused_by_name() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = reference_of_length(&scope.id(&format!("{FIXTURE_TYPE}:long-id:")), 7, IDENTIFIER_BOUND + 1);

    let refused = client
        .post_entity(&json!({"id": id, "type": FIXTURE_TYPE, "@context": [TEST_SUITE_CONTEXT]}))
        .send()
        .await
        .expect("create entity request failed");
    let problem = assert_bad_request_data(&refused, "creating an entity whose id is one octet past the bound");
    let detail = problem["detail"].as_str().expect("a problem detail carries a detail");
    assert!(
        detail.contains("'id'") && detail.contains("1025 bytes") && detail.contains("1024 bytes"),
        "the refusal names the member, its length and the bound: {detail}"
    );

    let batch = client
        .batch_upsert(&json!([{"id": id, "type": FIXTURE_TYPE, "@context": [TEST_SUITE_CONTEXT]}]))
        .await
        .expect("batch upsert request failed");
    assert_eq!(
        batch.status,
        StatusCode::MULTI_STATUS,
        "a batch reports the refusal per entity: {:?}",
        batch.body
    );
    let result = batch.json_value().expect("a 207 carries a BatchOperationResult");
    let error = &result["errors"][0];
    assert_eq!(error["entityId"], json!(id), "the error names the over-long id byte for byte: {result}");
    assert_eq!(error["error"]["type"], json!(BAD_REQUEST_DATA.as_str()), "{error}");
    let detail = error["error"]["detail"].as_str().expect("a problem detail carries a detail");
    assert!(
        detail.contains("'id'") && detail.contains("1024 bytes"),
        "the refusal names the member and the bound: {detail}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// A trip update whose `stopTimeUpdates` and an alert whose `informedEntities` hold references of 1047 to 5763 octets. Both are valid URIs, so the batch that carries them, and the valid neighbour beside them, is stored whole.
#[tokio::test]
async fn regression_long_uri_references_a_realtime_feed_with_kilobyte_references_is_stored_whole() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let trip_update = scope.id("GtfsRtTripUpdate:trip-1");
    let alert = scope.id("GtfsRtAlert:alert-1");
    let vehicle = scope.id("GtfsRtVehiclePosition:vehicle-1");
    let stop_time_updates: Vec<String> = [1047, 2311, 3988, 5763]
        .into_iter()
        .zip(10..)
        .map(|(length, seed)| reference_of_length("urn:ngsi-ld:GtfsRtStopTimeUpdate:feed:trip-1:", seed, length))
        .collect();
    let informed_entities: Vec<String> = [1500, 4200]
        .into_iter()
        .zip(20..)
        .map(|(length, seed)| reference_of_length("urn:ngsi-ld:GtfsRtInformedEntity:feed:alert-1:", seed, length))
        .collect();

    let batch = json!([
        {
            "id": trip_update,
            "type": "GtfsRtTripUpdate",
            "stopTimeUpdates": {"type": "ListRelationship", "objectList": stop_time_updates},
            "@context": [TEST_SUITE_CONTEXT]
        },
        {
            "id": alert,
            "type": "GtfsRtAlert",
            "informedEntities": {"type": "ListRelationship", "objectList": informed_entities},
            "@context": [TEST_SUITE_CONTEXT]
        },
        {
            "id": vehicle,
            "type": "GtfsRtVehiclePosition",
            "bearing": {"type": "Property", "value": 90},
            "@context": [TEST_SUITE_CONTEXT]
        }
    ]);
    let response = client.batch_upsert(&batch).await.expect("batch upsert request failed");
    assert!(
        response.status == StatusCode::CREATED || response.status == StatusCode::NO_CONTENT,
        "the whole feed snapshot must be stored: {} {:?}",
        response.status,
        response.body
    );

    assert_eq!(
        retrieve(client, &trip_update).await["stopTimeUpdates"]["objectList"],
        normalized_object_list(&stop_time_updates)
    );
    assert_eq!(
        retrieve(client, &alert).await["informedEntities"]["objectList"],
        normalized_object_list(&informed_entities)
    );
    assert_eq!(retrieve(client, &vehicle).await["bearing"]["value"], json!(90));

    scope.finish().await.expect("cleanup failed");
}
