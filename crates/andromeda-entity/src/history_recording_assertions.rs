//! Assertions over the Temporal Evolution a recorded write carries.
//!
//! Clauses 4.5.7 and 4.5.8 make every entry of an attribute's Temporal
//! Evolution an instance "as mandated by clause 4.5.2", so what a Core API write
//! records has to be the instance the write left in the Entity, not the
//! Fragment that asked for it. These helpers read both off one
//! [`AtomicWrite`]: the observation among its history records and the state
//! its entity record stores, each encoded as the expanded document the store
//! keeps.

use andromeda_context_commit::{atomic_write::AtomicWrite, history_record::HistoryRecord};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use pretty_assertions::assert_eq;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// The one observation a write records, encoded as stored, or `None` when it
/// records none.
pub(crate) fn recorded_observation(write: &AtomicWrite) -> Option<Value> {
    let inline = write.history.iter().filter_map(|record| match record {
        HistoryRecord::Observations(observed) => Some(serde_json::to_value(observed.as_ref()).expect("an observation encodes")),
        HistoryRecord::EvolutionInstances(observed) => Some(serde_json::to_value(observed.as_ref()).expect("an observation encodes")),
        HistoryRecord::Tombstone(_) | HistoryRecord::ScopeRemoval | HistoryRecord::ScopeAssertion(_) => None,
    });
    let observations: Vec<Value> = inline.collect();
    assert!(observations.len() <= 1, "a write records at most one observation: {observations:?}");
    observations.into_iter().next()
}

/// The attribute members an observation carries, by expanded name.
pub(crate) fn observed_attributes(observation: &Value) -> BTreeSet<String> {
    node(observation)
        .keys()
        .filter(|member| !member.starts_with('@') && member.as_str() != properties::SCOPE.as_str())
        .cloned()
        .collect()
}

/// Assert the write records exactly `attributes`, each observed as the
/// instances the Entity it stores holds for that attribute.
pub(crate) fn assert_observes_stored_instances(write: &AtomicWrite, attributes: &[&str]) {
    let observation = recorded_observation(write).expect("the write records an observation");
    let expected: BTreeSet<String> = attributes.iter().map(|attribute| (*attribute).to_owned()).collect();
    assert_eq!(
        observed_attributes(&observation),
        expected,
        "the observation names exactly the written attributes"
    );
    let stored = serde_json::to_value(write.entity.record().expect("the write stores an entity record").state.as_ref()).expect("the state encodes");
    for attribute in attributes {
        assert_eq!(
            node(&observation).get(*attribute),
            node(&stored).get(*attribute),
            "the observed {attribute} is the instance the Entity stores, not the request Fragment"
        );
    }
}

/// The observed instance of `attribute` carrying no `datasetId`.
pub(crate) fn observed_default_instance<'a>(observation: &'a Value, attribute: &str) -> &'a Value {
    node(observation)
        .get(attribute)
        .and_then(Value::as_array)
        .and_then(|instances| instances.iter().find(|instance| instance.get(properties::DATASET_ID.as_str()).is_none()))
        .expect("the observation carries the default instance of the attribute")
}

/// The Entity node of an encoded state or document.
fn node(encoded: &Value) -> &Map<String, Value> {
    match encoded {
        Value::Array(nodes) => nodes.first().and_then(Value::as_object),
        Value::Object(node) => Some(node),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }
    .expect("the encoded state is an expanded Entity node")
}
