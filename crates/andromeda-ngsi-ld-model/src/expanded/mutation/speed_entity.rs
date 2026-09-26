//! The multi-instance `speed` entity the mutation tests operate on, the reader
//! that flattens it back to the values it holds, and the Property instance
//! those tests build their payloads from.

use serde_json::{Value, json};

/// Entity with three `speed` instances: datasets `A`, `B`, and the default.
pub(crate) fn with_three_instances() -> Value {
    json!([{
        "@id": "urn:ngsi-ld:Vehicle:1",
        "https://example.com/speed": [
            {"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 10}],
             "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:A"}]},
            {"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 20}],
             "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:B"}]},
            {"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 30}]}
        ]
    }])
}

/// The `speed` values still present on the entity, in instance order.
pub(crate) fn values(entity: &Value) -> Vec<i64> {
    entity[0]["https://example.com/speed"]
        .as_array()
        .expect("speed instances")
        .iter()
        .filter_map(|instance| instance["https://uri.etsi.org/ngsi-ld/hasValue"][0]["@value"].as_i64())
        .collect()
}

/// One expanded Property instance carrying `value`, optionally tagged with a
/// `datasetId`. A `value` of `urn:ngsi-ld:null` is a deletion sentinel.
pub(crate) fn property_instance(value: &Value, dataset: Option<&str>) -> Value {
    let mut node = json!({
        "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
        "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]
    });
    if let Some(dataset) = dataset {
        node["https://uri.etsi.org/ngsi-ld/datasetId"] = json!([{"@id": dataset}]);
    }
    node
}
