//! Entity versions in the model, for the merge tests.

use andromeda_ngsi_ld_entity::{attribute_instances::AttributeInstances, entity_state::EntityState};
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use andromeda_ngsi_ld_model::entity_system_times::EntitySystemTimes;
use serde_json::{Map, Value, json};

/// The default vocabulary every fixture attribute is named in.
const DEFAULT_VOCABULARY: &str = "https://uri.etsi.org/ngsi-ld/default-context/";

/// The expanded name of the attribute a fixture calls `term`.
pub(crate) fn iri(term: &str) -> String {
    format!("{DEFAULT_VOCABULARY}{term}")
}

/// A version of the entity `id` holding, for each `(term, instances)`, the
/// Property instances `instances` lists, each `{"value", "observedAt"?,
/// "datasetId"?}`.
pub(crate) fn version(id: &str, attributes: &[(&str, Value)]) -> ReceivedState {
    let mut stored = Map::new();
    stored.insert("@id".to_owned(), json!(id));
    stored.insert("@type".to_owned(), json!([iri("T")]));
    for (term, instances) in attributes {
        let written: Vec<Value> = instances
            .as_array()
            .map_or_default(Vec::as_slice)
            .iter()
            .map(|instance| {
                let mut node = Map::new();
                node.insert("@type".to_owned(), json!(["https://uri.etsi.org/ngsi-ld/Property"]));
                node.insert("https://uri.etsi.org/ngsi-ld/hasValue".to_owned(), json!([{"@value": instance["value"]}]));
                if let Some(observed) = instance.get("observedAt") {
                    node.insert(
                        "https://uri.etsi.org/ngsi-ld/observedAt".to_owned(),
                        json!([{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": observed}]),
                    );
                }
                if let Some(dataset) = instance.get("datasetId") {
                    node.insert("https://uri.etsi.org/ngsi-ld/datasetId".to_owned(), json!([{"@id": dataset}]));
                }
                Value::Object(node)
            })
            .collect();
        stored.insert(iri(term), Value::Array(written));
    }
    let state = EntityState::from_slice(Value::Array(vec![Value::Object(stored)]).to_string().as_bytes()).expect("a stored state");
    ReceivedState {
        state,
        system_times: EntitySystemTimes::default(),
        linked: Vec::new(),
    }
}

/// How many instances the version holds of the attribute `term`.
pub(crate) fn instance_count(version: &ReceivedState, term: &str) -> usize {
    version.state.attributes().get(&iri(term)).map_or(0, AttributeInstances::len)
}
