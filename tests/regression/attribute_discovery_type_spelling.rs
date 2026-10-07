//! ETSI GS CIM 009 v1.9.1 clause 5.7.9 has Retrieve Available Attributes answer "an Attribute List as mandated by clause 5.2.28", and clause 5.5.6 admits no `InternalError` for a well-formed read of data the broker itself stored.
//!
//! JSON-LD 1.1 section 4.1 admits both spellings on a node object -- "the value of @type ... MUST be either a string or an array of strings" -- and only expansion normalises it to the array form, so a stored node carrying the string spelling is well-formed.
//!
//! Discovery answers from the census the entity writes keep, which records the family each instance declared when it was written (clause 4.5.2 makes an instance without one a Property). A payload rewritten behind the broker therefore can neither fail a listing nor change what it reports. Each test writes that spelling into the stored payload directly, because the ingest path expands what it is given and so cannot produce it through the API.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// The expanded `@type` an NGSI-LD Property node carries (clause 4.5.2).
const PROPERTY_IRI: &str = "https://uri.etsi.org/ngsi-ld/Property";

/// The core context every payload here is written against; every name this
/// module uses is already a full IRI, so nothing else has to be declared.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// The vocabulary one run declares, unique per run so a shared tenant cannot let two runs contribute to each other's answers.
struct RunVocabulary {
    /// The run-unique token every IRI is built from.
    token: String,
}

impl RunVocabulary {
    /// Allocate a vocabulary for one run.
    fn allocate() -> RunVocabulary {
        RunVocabulary {
            token: Uuid::new_v4().simple().to_string(),
        }
    }

    /// The IRI an attribute of this run is stored under.
    fn attribute(&self, base: &str) -> String {
        format!("http://example.org/regression/{}/{base}", self.token)
    }

    /// The IRI the entity type of this run is stored under.
    fn entity_type(&self) -> String {
        self.attribute("Probe")
    }
}

/// One entity carrying two Properties named by their full IRIs, so what the
/// discovery aggregate groups on is exactly what this module asserts about.
fn entity(id: &str, vocabulary: &RunVocabulary) -> Value {
    json!({
        "id": id,
        "type": vocabulary.entity_type(),
        vocabulary.attribute("stated"): {"type": "Property", "value": 1},
        vocabulary.attribute("neighbour"): {"type": "Property", "value": 2},
        "@context": CORE_CONTEXT
    })
}

/// Create one entity and assert clause 6.4.3.1 answered 201.
async fn create(client: &TestClient, body: &Value) {
    let response = client.post_entity(body).with_ld_json().send().await.expect("create entity request");
    assert_eq!(response.status, StatusCode::CREATED, "create {}: {:?}", body["id"], response.body);
}

/// The expanded payload is an array holding one node object, so the Attribute node is at `payload -> 0 -> <attribute IRI> -> 0`.
async fn store_attribute_type(pool: &PgPool, id: &str, attribute: &str, spelling: &Value) {
    let updated = sqlx::query(
        "UPDATE entity_documents AS d \
         SET payload = jsonb_set(d.payload, ARRAY['0', $2, '0', '@type'], $3::jsonb) \
         FROM entities AS e \
         WHERE e.entity_key = d.entity_key AND e.id = $1",
    )
    .bind(id)
    .bind(attribute)
    .bind(spelling.to_string())
    .execute(pool)
    .await
    .expect("rewrite the stored attribute type");
    assert_eq!(updated.rows_affected(), 1, "exactly one stored payload names {id}");
}

/// The Attribute List of clause 5.7.9.
async fn attribute_list(client: &TestClient) -> (StatusCode, Value) {
    read(client, "/ngsi-ld/v1/attributes").await
}

/// The Attribute of clause 5.7.10, for one attribute IRI.
async fn attribute_info(client: &TestClient, attribute: &str) -> (StatusCode, Value) {
    read(client, &format!("/ngsi-ld/v1/attributes/{}", urlencoding::encode(attribute))).await
}

/// One `application/json` read.
async fn read(client: &TestClient, path: &str) -> (StatusCode, Value) {
    let response = client
        .raw(Method::GET, path)
        .header("Accept", "application/json")
        .send()
        .await
        .expect("discovery read request");
    let body = response.json_value().expect("a discovery read answers with JSON");
    (response.status, body)
}

/// Whether the Attribute List names `attribute`.
fn lists(body: &Value, attribute: &str) -> bool {
    body["attributeList"].as_array().is_some_and(|names| names.iter().any(|name| name == attribute))
}

/// Clause 5.7.9: the listing answers an Attribute List, and the string spelling
/// of `@type` is read as the one type it names rather than failing the read.
#[tokio::test]
async fn regression_attribute_discovery_a_string_spelled_at_type_is_read_as_its_one_family() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let vocabulary = RunVocabulary::allocate();
    let id = scope.id("Probe:attribute-type-string-spelling");
    create(client, &entity(&id, &vocabulary)).await;
    let stated = vocabulary.attribute("stated");
    store_attribute_type(scope.database_pool(), &id, &stated, &json!(PROPERTY_IRI)).await;

    let (status, body) = attribute_list(client).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(lists(&body, &stated), "{body}");

    let (info_status, info) = attribute_info(client, &stated).await;
    assert_eq!(info_status, StatusCode::OK, "{info}");
    assert_eq!(info["attributeTypes"], json!(["Property"]), "{info}");

    scope.finish().await.expect("cleanup failed");
}

/// The expansion is cross-joined into the aggregate, so one node that failed it
/// would fail every group of the listing. Nothing about one attribute may decide
/// whether another is reported.
#[tokio::test]
async fn regression_attribute_discovery_one_node_never_decides_another_attributes_fate() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let vocabulary = RunVocabulary::allocate();
    let id = scope.id("Probe:attribute-type-neighbour");
    create(client, &entity(&id, &vocabulary)).await;
    let stated = vocabulary.attribute("stated");
    let neighbour = vocabulary.attribute("neighbour");
    store_attribute_type(scope.database_pool(), &id, &stated, &json!(PROPERTY_IRI)).await;

    let (status, body) = attribute_list(client).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(lists(&body, &neighbour), "the untouched attribute is still listed: {body}");

    let (info_status, info) = attribute_info(client, &neighbour).await;
    assert_eq!(info_status, StatusCode::OK, "{info}");
    assert_eq!(info["attributeTypes"], json!(["Property"]), "{info}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.2.28 makes `attributeTypes` the families the instances declare, and the census records the family each write declared, so a stored `@type` rewritten behind the broker neither fails the listing nor changes the family reported.
#[tokio::test]
async fn regression_attribute_discovery_an_unreadable_at_type_changes_no_answer() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let vocabulary = RunVocabulary::allocate();
    let stated = vocabulary.attribute("stated");

    for (local, spelling) in [
        ("Probe:attribute-type-object", json!({"not": "a type"})),
        ("Probe:attribute-type-number", json!(42)),
        ("Probe:attribute-type-null", Value::Null),
        ("Probe:attribute-type-boolean", json!(true)),
    ] {
        let id = scope.id(local);
        create(client, &entity(&id, &vocabulary)).await;
        store_attribute_type(scope.database_pool(), &id, &stated, &spelling).await;

        let (status, body) = attribute_list(client).await;
        assert_eq!(status, StatusCode::OK, "{spelling} listed: {body}");
        assert!(lists(&body, &stated), "{spelling} listed: {body}");

        let (info_status, info) = attribute_info(client, &stated).await;
        assert_eq!(info_status, StatusCode::OK, "{spelling}: {info}");
        assert_eq!(info["attributeTypes"], json!(["Property"]), "{spelling}: {info}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// An empty `@type` array is the third spelling a listing has to be total
/// over; the instance is still counted, as the one it was written as.
#[tokio::test]
async fn regression_attribute_discovery_an_empty_at_type_array_names_no_family() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let vocabulary = RunVocabulary::allocate();
    let id = scope.id("Probe:attribute-type-empty-array");
    create(client, &entity(&id, &vocabulary)).await;
    let stated = vocabulary.attribute("stated");
    store_attribute_type(scope.database_pool(), &id, &stated, &json!([])).await;

    let (status, body) = attribute_list(client).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(lists(&body, &stated), "{body}");

    let (info_status, info) = attribute_info(client, &stated).await;
    assert_eq!(info_status, StatusCode::OK, "{info}");
    assert_eq!(info["attributeCount"], json!(1), "{info}");

    scope.finish().await.expect("cleanup failed");
}

/// Clauses 5.7.5 to 5.7.7 aggregate over the attribute keys rather than the `@type` of each node, so the spelling cannot reach them.
#[tokio::test]
async fn regression_attribute_discovery_the_type_listing_is_unaffected_by_the_spelling() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let vocabulary = RunVocabulary::allocate();
    let id = scope.id("Probe:attribute-type-types-listing");
    create(client, &entity(&id, &vocabulary)).await;
    store_attribute_type(scope.database_pool(), &id, &vocabulary.attribute("stated"), &json!(PROPERTY_IRI)).await;

    let (status, body) = read(client, "/ngsi-ld/v1/types?details=true").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let details = body["typeList"].as_array().or_else(|| body.as_array()).expect("a type list: {body}");
    assert!(
        details
            .iter()
            .any(|entry| entry["typeName"] == json!(vocabulary.entity_type()) || entry["id"] == json!(vocabulary.entity_type())),
        "{body}"
    );

    scope.finish().await.expect("cleanup failed");
}
