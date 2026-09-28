//! Assertions over the Temporal Evolution a recorded write carries.
//!
//! Clauses 4.5.7 and 4.5.8 make every entry of an attribute's Temporal
//! Evolution an instance "as mandated by clause 4.5.2", so what a Core API write
//! records has to be the instance the write left in the Entity, not the
//! Fragment that asked for it. These helpers read both off one
//! [`AtomicWrite`]: the observation among its history records, or deferred to
//! the side-effect job on the asynchronous batch path, and the payload its
//! entity record stores.

use andromeda_context_commit::{atomic_write::AtomicWrite, history_record::HistoryRecord};
use andromeda_ngsi_ld_model::expanded::node::expanded_object_ref;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use pretty_assertions::assert_eq;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// The one observation a write records, inline or deferred to its side-effect
/// job, or `None` when it records none.
pub(crate) fn recorded_observation(write: &AtomicWrite) -> Option<&Value> {
    let inline = write.history.iter().filter_map(|record| match record {
        HistoryRecord::Observations(observed) | HistoryRecord::EvolutionInstances(observed) => Some(observed.as_ref()),
        HistoryRecord::Tombstone(_) | HistoryRecord::ScopeRemoval => None,
    });
    let deferred = write
        .side_effects
        .iter()
        .flat_map(|job| job.temporal_entities.iter().map(|(_, observed)| observed.as_ref()));
    let observations: Vec<&Value> = inline.chain(deferred).collect();
    assert!(observations.len() <= 1, "a write records at most one observation: {observations:?}");
    observations.first().copied()
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
        observed_attributes(observation),
        expected,
        "the observation names exactly the written attributes"
    );
    let stored = &write.entity.record().expect("the write stores an entity record").payload;
    for attribute in attributes {
        assert_eq!(
            node(observation).get(*attribute),
            node(stored).get(*attribute),
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

/// The expanded Entity node of a payload.
fn node(payload: &Value) -> &Map<String, Value> {
    expanded_object_ref(payload).expect("the payload is an expanded Entity node")
}
