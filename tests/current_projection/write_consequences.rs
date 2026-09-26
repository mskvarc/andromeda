//! What a Core API write leaves behind, read through the three surfaces that
//! must agree on it: the Entity itself, its Temporal Evolution, and the
//! current-value projection a `q` filter is answered from.
//!
//! Clauses 4.5.7 and 4.5.8 make every entry of an attribute's Temporal
//! Evolution an instance "as mandated by clause 4.5.2", and the projection is
//! written from the same interpretation of the write. Both therefore have to
//! hold the instance the write left in the Entity, not the Fragment that asked
//! for it. Requests carry the NGSI-LD Core Context alone, so every term expands
//! through its `@vocab` and no remote context has to be reachable.

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use urlencoding::encode;

/// The Core Context every request in these rows carries.
pub fn core_context() -> Value {
    json!([CORE_CONTEXT.as_str()])
}

/// Create one Entity, failing the test unless the broker answers 201.
pub async fn create(client: &TestClient, entity: &Value) {
    let created = client.post_entity(entity).send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// Send a raw `application/ld+json` write and assert the status it answers.
pub async fn write(client: &TestClient, method: Method, path: &str, body: &Value, expected: StatusCode) {
    let response = client
        .raw(method, &format!("/ngsi-ld/v1/{path}"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("write request");
    assert_eq!(response.status, expected, "{path}: {:?}", response.body);
}

/// The attribute of one Entity as the Core API renders it now.
pub async fn current_attribute(client: &TestClient, entity_id: &str, attribute: &str) -> Value {
    let response = client
        .get_entity(entity_id, "", "application/json", None)
        .await
        .expect("retrieve entity request");
    assert_eq!(response.status, StatusCode::OK, "retrieve: {:?}", response.body);
    response.json_value().expect("entity body")[attribute].clone()
}

/// Every instance of one attribute's Temporal Evolution (clause 5.7.3),
/// whichever of the two shapes clause 4.5.7 renders it in.
pub async fn temporal_instances(client: &TestClient, entity_id: &str, attribute: &str) -> Vec<Value> {
    let response = client
        .get_temporal_entity(entity_id, &format!("attrs={attribute}"), None)
        .await
        .expect("temporal retrieval request");
    assert_eq!(response.status, StatusCode::OK, "temporal retrieval: {:?}", response.body);
    match response.json_value().expect("temporal body").get(attribute) {
        Some(Value::Array(instances)) => instances.clone(),
        Some(instance) => vec![instance.clone()],
        None => Vec::new(),
    }
}

/// The attribute names the Temporal Evolution of one Entity carries.
pub async fn temporal_attribute_names(client: &TestClient, entity_id: &str) -> Vec<String> {
    let response = client.get_temporal_entity(entity_id, "", None).await.expect("temporal retrieval request");
    assert_eq!(response.status, StatusCode::OK, "temporal retrieval: {:?}", response.body);
    let body = response.json_value().expect("temporal body");
    let mut names: Vec<String> = body
        .as_object()
        .expect("a temporal Entity is an object")
        .keys()
        .filter(|member| !matches!(member.as_str(), "id" | "type" | "@context" | "scope"))
        .cloned()
        .collect();
    names.sort();
    names
}

/// The one Temporal Evolution instance of an attribute holding `value`.
pub fn instance_valued<'a>(instances: &'a [Value], value: &Value) -> &'a Value {
    let matching: Vec<&Value> = instances.iter().filter(|instance| instance.get("value") == Some(value)).collect();
    assert_eq!(matching.len(), 1, "exactly one recorded instance holds {value}: {instances:?}");
    matching[0]
}

/// The instances of an attribute's Temporal Evolution that carry `dataset_id`.
pub fn instances_of_dataset<'a>(instances: &'a [Value], dataset_id: &str) -> Vec<&'a Value> {
    instances
        .iter()
        .filter(|instance| instance.get("datasetId").and_then(Value::as_str) == Some(dataset_id))
        .collect()
}

/// Whether a clause 4.9 `q` filter over one Entity type selects the Entity.
pub async fn q_selects(client: &TestClient, entity_type: &str, entity_id: &str, q: &str) -> bool {
    let response = client
        .query_entities(&format!("type={entity_type}&q={}", encode(q)), "application/json", None)
        .await
        .expect("query request");
    assert_eq!(response.status, StatusCode::OK, "query {q}: {:?}", response.body);
    response
        .json_value()
        .expect("query body")
        .as_array()
        .expect("a query answers an array")
        .iter()
        .any(|entity| entity.get("id").and_then(Value::as_str) == Some(entity_id))
}
