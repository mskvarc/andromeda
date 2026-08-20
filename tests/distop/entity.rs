//! Entity and attribute JSON builders shared across the distributed-operations
//! areas, so provision and consumption tests describe payloads by intent rather
//! than by hand-assembled JSON.

use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use serde_json::{Map, Value, json};

/// A normalized `Property` node with the given value.
pub fn property(value: Value) -> Value {
    json!({ "type": "Property", "value": value })
}

/// A normalized `Relationship` node pointing at the given object id.
pub fn relationship(object: &str) -> Value {
    json!({ "type": "Relationship", "object": object })
}

/// A `Property` carrying a `datasetId`, for multi-instance attribute tests.
pub fn property_with_dataset(value: Value, dataset_id: &str) -> Value {
    json!({ "type": "Property", "value": value, "datasetId": dataset_id })
}

/// A minimal `Vehicle` entity holding a single `speed` property, with the
/// compound context inline for `application/ld+json` submission.
pub fn vehicle(id: &str, speed: i64) -> Value {
    entity(id, "Vehicle", &[("speed", property(json!(speed)))])
}

/// An entity of the given type holding the named attributes, context inline.
pub fn entity(id: &str, entity_type: &str, attributes: &[(&str, Value)]) -> Value {
    let mut body = Map::new();
    body.insert("id".to_owned(), json!(id));
    body.insert("type".to_owned(), json!(entity_type));
    for (name, node) in attributes {
        body.insert((*name).to_owned(), node.clone());
    }
    body.insert("@context".to_owned(), json!(COMPOUND_CONTEXT.as_str()));
    Value::Object(body)
}

/// An attribute fragment (no `id`/`type`/`@context` envelope besides context),
/// for append/update operations on an existing entity.
pub fn attribute_fragment(attributes: &[(&str, Value)]) -> Value {
    let mut body = Map::new();
    for (name, node) in attributes {
        body.insert((*name).to_owned(), node.clone());
    }
    body.insert("@context".to_owned(), json!(COMPOUND_CONTEXT.as_str()));
    Value::Object(body)
}
