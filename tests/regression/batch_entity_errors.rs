//! ETSI GS CIM 009 v1.9.1 clauses 5.6.7.4 to 5.6.10.4 and 5.6.20.4 run the single-entity operation for each entity of the input array and, when it fails, add "a new `BatchEntityError` [...] containing the failed Entity ID and the related `ProblemDetails`" to the result; clause 5.6.8.4 says it most broadly: "If while processing an Entity there is any kind of error or abnormal situation, a `BatchEntityError` shall be added to E". Tables 6.14.3.1-1 and following answer 207 "if only some or none of the entities have been successfully" processed. The request fails as a whole only when there is no entity to report against: a null item (step 1 of each clause), and an item whose `id` is absent or not a URI, because Table 5.2.17-1 types `entityId` as a valid URI. Clause 5.5.4 makes each of those `BadRequestData`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{problem_details::validate_problem_details, response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::{context::TEST_SUITE_CONTEXT, problem_detail::BAD_REQUEST_DATA};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The entity type every fixture in this module carries.
const FIXTURE_TYPE: &str = "BatchEntityError";

/// The identifier bound, in octets, that every Entity id is held to.
const IDENTIFIER_BOUND: usize = 1024;

/// A URN of exactly one octet past the identifier bound.
fn over_long_id(prefix: &str) -> String {
    let id = format!("{prefix}{}", "a".repeat(IDENTIFIER_BOUND + 1 - prefix.len()));
    assert_eq!(id.len(), IDENTIFIER_BOUND + 1, "the fixture is one octet past the bound");
    id
}

/// An entity with a `speed` Property of `value`.
fn entity(id: &str, value: &Value) -> Value {
    json!({
        "id": id,
        "type": FIXTURE_TYPE,
        "speed": {"type": "Property", "value": value},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// An entity whose `speed` Property carries `unit_code`.
fn entity_with_unit(id: &str, unit_code: &str) -> Value {
    json!({
        "id": id,
        "type": FIXTURE_TYPE,
        "speed": {"type": "Property", "value": 1, "unitCode": unit_code},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// An entity whose `speed` Property has a null `value`, which clause 5.5.4 refuses.
fn entity_with_null_value(id: &str) -> Value {
    entity(id, &Value::Null)
}

/// The clause 5.2.16 result of a 207 answer.
fn multi_status(response: &TestResponse, operation: &str) -> Value {
    assert_eq!(response.status, StatusCode::MULTI_STATUS, "{operation}: {:?}", response.body);
    response.json_value().expect("a 207 carries a BatchOperationResult")
}

/// The `success` identifiers of a clause 5.2.16 result, in order.
fn successes(result: &Value) -> Vec<&str> {
    result["success"]
        .as_array()
        .expect("a result carries a success array")
        .iter()
        .filter_map(Value::as_str)
        .collect()
}

/// The error a clause 5.2.16 result reports against `entity_id`.
fn error_against<'a>(result: &'a Value, entity_id: &str) -> &'a Value {
    result["errors"]
        .as_array()
        .expect("a result carries an errors array")
        .iter()
        .find(|error| error["entityId"].as_str() == Some(entity_id))
        .expect("an error is reported against the entity")
}

/// Assert that the error against `entity_id` is `BadRequestData`, and return
/// its detail.
fn bad_request_data_against<'a>(result: &'a Value, entity_id: &str) -> &'a str {
    let error = error_against(result, entity_id);
    assert_eq!(error["error"]["type"], json!(BAD_REQUEST_DATA.as_str()), "{error}");
    error["error"]["detail"].as_str().expect("a problem detail carries a detail")
}

/// Assert that the request failed as a whole with `BadRequestData`, and return
/// the problem's detail.
fn refused_as_bad_request_data(response: &TestResponse, operation: &str) -> String {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{operation}: {:?}", response.body);
    let problem = response.json_value().expect("problem details must be JSON");
    let verdict = validate_problem_details(&problem, BAD_REQUEST_DATA);
    assert!(verdict.is_ok(), "{operation} must be refused as BadRequestData: {verdict:?} {problem}");
    problem["detail"].as_str().expect("a problem detail carries a detail").to_owned()
}

/// The status a retrieval of `id` answers.
async fn retrieval_status(client: &TestClient, id: &str) -> StatusCode {
    client
        .get_entity(id, "", "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve entity request failed")
        .status
}

/// The `speed` value `id` holds.
async fn speed_of(client: &TestClient, id: &str) -> Value {
    let response = client
        .get_entity(id, "options=keyValues", "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve entity request failed");
    assert_eq!(response.status, StatusCode::OK, "retrieve {id}: {:?}", response.body);
    response.json_value().expect("an entity must be JSON")["speed"].clone()
}

/// Assert that an over-long `id`'s refusal names the member, its length and
/// the bound.
fn assert_names_the_bound(detail: &str) {
    assert!(
        detail.contains("'id'") && detail.contains("1025 bytes") && detail.contains("1024 bytes"),
        "the refusal names the member, its length and the bound: {detail}"
    );
}

/// Clause 5.6.7.4: a batch create of a valid entity, one whose `id` is one
/// octet past the identifier bound, and one with a null `value` creates the
/// valid one and reports the other two against their own `id`s.
#[tokio::test]
async fn regression_batch_entity_errors_create_reports_each_refused_entity_and_creates_the_rest() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let valid = scope.id(&format!("{FIXTURE_TYPE}:create-valid"));
    let long = over_long_id(&scope.id(&format!("{FIXTURE_TYPE}:create-long:")));
    let null_value = scope.id(&format!("{FIXTURE_TYPE}:create-null-value"));

    let response = client
        .batch_create(&json!([
            entity(&valid, &json!(1)),
            entity(&long, &json!(2)),
            entity_with_null_value(&null_value)
        ]))
        .await
        .expect("batch create request failed");
    let result = multi_status(&response, "a batch create with two refused entities");
    assert_eq!(successes(&result), vec![valid.as_str()]);
    assert_eq!(result["errors"].as_array().map(Vec::len), Some(2), "{result}");
    assert_names_the_bound(bad_request_data_against(&result, &long));
    bad_request_data_against(&result, &null_value);

    assert_eq!(retrieval_status(client, &valid).await, StatusCode::OK);
    assert_eq!(retrieval_status(client, &null_value).await, StatusCode::NOT_FOUND);

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.8.4: the same batch upserted stores the valid entity.
#[tokio::test]
async fn regression_batch_entity_errors_upsert_reports_each_refused_entity_and_stores_the_rest() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let valid = scope.id(&format!("{FIXTURE_TYPE}:upsert-valid"));
    let long = over_long_id(&scope.id(&format!("{FIXTURE_TYPE}:upsert-long:")));
    let null_value = scope.id(&format!("{FIXTURE_TYPE}:upsert-null-value"));

    let response = client
        .batch_upsert(&json!([
            entity(&valid, &json!(1)),
            entity(&long, &json!(2)),
            entity_with_null_value(&null_value)
        ]))
        .await
        .expect("batch upsert request failed");
    let result = multi_status(&response, "a batch upsert with two refused entities");
    assert_eq!(successes(&result), vec![valid.as_str()]);
    assert_names_the_bound(bad_request_data_against(&result, &long));
    bad_request_data_against(&result, &null_value);
    assert_eq!(retrieval_status(client, &valid).await, StatusCode::OK);

    scope.finish().await.expect("cleanup failed");
}

/// Clauses 5.6.9.4 and 5.6.20.4: a batch update and a batch merge apply the
/// valid entity and leave the refused one as it was.
#[tokio::test]
async fn regression_batch_entity_errors_update_and_merge_apply_the_valid_entity_and_leave_the_refused_one() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let valid = scope.id(&format!("{FIXTURE_TYPE}:update-valid"));
    let refused = scope.id(&format!("{FIXTURE_TYPE}:update-refused"));
    let long = over_long_id(&scope.id(&format!("{FIXTURE_TYPE}:update-long:")));

    let seeded = client
        .batch_create(&json!([entity(&valid, &json!(1)), entity(&refused, &json!(1))]))
        .await
        .expect("batch create request failed");
    assert_eq!(seeded.status, StatusCode::CREATED, "seeding: {:?}", seeded.body);

    for (operation, value) in [("update", 2), ("merge", 3)] {
        let body = json!([entity(&valid, &json!(value)), entity(&long, &json!(value)), entity_with_null_value(&refused)]);
        let response = match operation {
            "update" => client.batch_update(&body).await,
            _merge => client.batch_merge(&body).await,
        }
        .expect("batch request failed");
        let result = multi_status(&response, operation);
        assert_eq!(successes(&result), vec![valid.as_str()], "{operation}");
        assert_names_the_bound(bad_request_data_against(&result, &long));
        bad_request_data_against(&result, &refused);
        assert_eq!(speed_of(client, &valid).await, json!(value), "{operation} applies the valid entity");
        assert_eq!(speed_of(client, &refused).await, json!(1), "{operation} leaves the refused entity unchanged");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Table 6.14.3.1-1: 207 "if only some or none of the entities have been successfully created", so a batch whose every entity is refused is still a 207 rather than a 400.
#[tokio::test]
async fn regression_batch_entity_errors_a_batch_in_which_every_entity_is_refused_is_a_207() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let long = over_long_id(&scope.id(&format!("{FIXTURE_TYPE}:none-long:")));
    let null_value = scope.id(&format!("{FIXTURE_TYPE}:none-null-value"));

    let response = client
        .batch_create(&json!([entity(&long, &json!(1)), entity_with_null_value(&null_value)]))
        .await
        .expect("batch create request failed");
    let result = multi_status(&response, "a batch create in which every entity is refused");
    assert!(successes(&result).is_empty(), "{result}");
    assert_eq!(result["errors"].as_array().map(Vec::len), Some(2), "{result}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.8.4 step 1 and Tables 5.2.4-1 and 5.2.17-1: a null item, an item without an `id` and an item whose `id` is not a URI leave nothing to report against, so each fails the whole request as `BadRequestData` -- not `InvalidRequest`, which clause 5.5.4 keeps for a body that is not JSON -- and nothing of the batch is stored.
#[tokio::test]
async fn regression_batch_entity_errors_an_item_with_no_uri_to_report_against_fails_the_request_and_stores_nothing() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let valid = scope.id(&format!("{FIXTURE_TYPE}:whole-request-valid"));

    for (item, expected) in [
        (Value::Null, "batch item 1 is null"),
        (
            json!({"type": FIXTURE_TYPE, "@context": [TEST_SUITE_CONTEXT]}),
            "batch item 1 has no 'id' string",
        ),
        (
            json!({"id": "not a uri", "type": FIXTURE_TYPE, "@context": [TEST_SUITE_CONTEXT]}),
            "the 'id' of batch item 1 is not a URI: invalid URI: not a valid URN, URL or IRI",
        ),
    ] {
        let response = client
            .batch_upsert(&json!([entity(&valid, &json!(1)), item]))
            .await
            .expect("batch upsert request failed");
        let detail = refused_as_bad_request_data(&response, expected);
        assert_eq!(detail, expected);
        assert_eq!(retrieval_status(client, &valid).await, StatusCode::NOT_FOUND, "{expected}: nothing is stored");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.10.4: a batch delete of an existing entity and an `id` one octet
/// past the identifier bound deletes the entity and reports the other against
/// itself.
#[tokio::test]
async fn regression_batch_entity_errors_delete_reports_an_over_long_id_and_deletes_the_rest() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let existing = scope.id(&format!("{FIXTURE_TYPE}:delete-existing"));
    let long = over_long_id(&scope.id(&format!("{FIXTURE_TYPE}:delete-long:")));

    let seeded = client
        .batch_create(&json!([entity(&existing, &json!(1))]))
        .await
        .expect("batch create request failed");
    assert_eq!(seeded.status, StatusCode::CREATED, "seeding: {:?}", seeded.body);

    let response = client.batch_delete(&json!([existing, long])).await.expect("batch delete request failed");
    let result = multi_status(&response, "a batch delete with an over-long id");
    assert_eq!(successes(&result), vec![existing.as_str()]);
    assert_names_the_bound(bad_request_data_against(&result, &long));
    assert_eq!(retrieval_status(client, &existing).await, StatusCode::NOT_FOUND);

    scope.finish().await.expect("cleanup failed");
}

/// Clauses 5.6.10.3 and 5.6.10.4 and Table 5.2.17-1: a batch delete item that is null, not a string, or not a URI fails the request as `BadRequestData`, and nothing is deleted.
#[tokio::test]
async fn regression_batch_entity_errors_a_delete_item_with_no_uri_fails_the_request_and_deletes_nothing() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let existing = scope.id(&format!("{FIXTURE_TYPE}:delete-kept"));

    let seeded = client
        .batch_create(&json!([entity(&existing, &json!(1))]))
        .await
        .expect("batch create request failed");
    assert_eq!(seeded.status, StatusCode::CREATED, "seeding: {:?}", seeded.body);

    for (item, expected) in [
        (Value::Null, "batch item 1 is null"),
        (json!(42), "batch item 1 is not an entity id string"),
        (
            json!("not a uri"),
            "the 'id' of batch item 1 is not a URI: invalid URI: not a valid URN, URL or IRI",
        ),
    ] {
        let response = client.batch_delete(&json!([existing, item])).await.expect("batch delete request failed");
        assert_eq!(refused_as_bad_request_data(&response, expected), expected);
        assert_eq!(retrieval_status(client, &existing).await, StatusCode::OK, "{expected}: nothing is deleted");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.5-1 restricts `unitCode` to a UN/CEFACT common code: an entity whose Property carries `km/h` is refused on its own, and its neighbour is stored with its unit.
#[tokio::test]
async fn regression_batch_entity_errors_a_unit_that_is_no_common_code_refuses_only_its_entity() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let valid = scope.id(&format!("{FIXTURE_TYPE}:unit-valid"));
    let refused = scope.id(&format!("{FIXTURE_TYPE}:unit-refused"));

    let response = client
        .batch_upsert(&json!([entity_with_unit(&valid, "KMH"), entity_with_unit(&refused, "km/h")]))
        .await
        .expect("batch upsert request failed");
    let result = multi_status(&response, "a batch upsert with one unusable unitCode");
    assert_eq!(successes(&result), vec![valid.as_str()]);
    let detail = bad_request_data_against(&result, &refused);
    assert!(
        detail.contains("'unitCode'") && detail.contains("not a UN/CEFACT common code"),
        "the refusal names the member: {detail}"
    );

    let stored = client
        .get_entity(&valid, "", "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve entity request failed");
    assert_eq!(stored.status, StatusCode::OK, "{:?}", stored.body);
    assert_eq!(stored.json_value().expect("an entity must be JSON")["speed"]["unitCode"], json!("KMH"));
    assert_eq!(retrieval_status(client, &refused).await, StatusCode::NOT_FOUND);

    scope.finish().await.expect("cleanup failed");
}

/// A batch larger than one commit chunk (512 writes) with one unusable unit in its first chunk stores every other entity: the refusal is the entity's, not the chunk's.
#[tokio::test]
async fn regression_batch_entity_errors_one_unusable_unit_in_a_multi_chunk_batch_loses_no_neighbour() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let ids: Vec<String> = (0..600).map(|index| scope.id(&format!("{FIXTURE_TYPE}:chunk-{index}"))).collect();
    let refused_index = 299;

    let body: Vec<Value> = ids
        .iter()
        .enumerate()
        .map(|(index, id)| entity_with_unit(id, if index == refused_index { "Celsius" } else { "CEL" }))
        .collect();
    let response = client.batch_upsert(&Value::Array(body)).await.expect("batch upsert request failed");
    let result = multi_status(&response, "a two-chunk batch upsert with one unusable unitCode");
    assert_eq!(successes(&result).len(), 599, "every other entity is stored");
    assert_eq!(result["errors"].as_array().map(Vec::len), Some(1), "{}", result["errors"]);
    bad_request_data_against(&result, &ids[refused_index]);

    for index in [0, refused_index - 1, refused_index + 1, 511, 512, 599] {
        assert_eq!(retrieval_status(client, &ids[index]).await, StatusCode::OK, "entity {index} is stored");
    }
    assert_eq!(retrieval_status(client, &ids[refused_index]).await, StatusCode::NOT_FOUND);

    scope.finish().await.expect("cleanup failed");
}
