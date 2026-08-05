//! The multi-instance `speed` entity the mutation tests operate on, and the
//! reader that flattens it back to the values it holds.

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
