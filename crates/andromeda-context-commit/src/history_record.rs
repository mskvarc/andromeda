//! Temporal Evolution consequences carried by an atomic write.

use crate::error::CommitError;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, uri::Uri};
use andromeda_ngsi_ld_model::expanded::{
    instance_key::{AttributeInstanceKey, attribute_instance_families},
    null_sentinel::without_null_sentinels,
    removed_instance::{RemovedAttributeInstance, removed_attribute_instances},
    scope_member::extract_scopes,
};
use derive_more::IsVariant;
use serde_json::Value;
use std::sync::Arc;

/// One temporal-history consequence of an entity mutation.
#[derive(Debug, IsVariant)]
pub enum HistoryRecord {
    /// Attribute observations contained in an expanded payload.
    Observations(Arc<Value>),
    /// End of one attribute instance that left current entity state.
    Tombstone(RemovedAttributeInstance),
    /// End of the Entity's Scope, which left current entity state.
    ///
    /// Clause 4.18: "For the Temporal Evolution a given Scope is considered
    /// valid from the time it has been set until the time it has been
    /// explicitly removed by an update or delete operation." A write that
    /// leaves the Entity with no Scope is such an operation — a Replace Entity
    /// (clause 5.6.18.4: "completely replace the existing Entity ... with the
    /// new Entity content provided") that omits `scope`, or the deletion of the
    /// Entity itself — and without this record the reconstructed interval would
    /// stay open for ever and report a Scope the Entity has not been in since.
    ///
    /// It carries nothing, because clause 4.18 gives an Entity at most one
    /// instance of the scope Property: there is only ever one Scope value to
    /// end, and `chk_scope_history_value_presence` forbids the stored removal
    /// from naming it.
    ScopeRemoval,
}

/// Build a tombstone for one validated attribute-instance identity.
///
/// The family travels with the record because a deletion renders the
/// `urn:ngsi-ld:null` member of the family the instance belonged to (clauses
/// 4.5.7 and 4.5.8), and current state no longer holds the instance to ask.
#[must_use]
pub fn tombstone(attr: &AttributeName, dataset_id: Option<&Uri>, attribute_type: AttributeType) -> HistoryRecord {
    HistoryRecord::Tombstone(RemovedAttributeInstance {
        key: AttributeInstanceKey {
            attr: attr.clone(),              // clone: the history record owns its attribute name.
            dataset_id: dataset_id.cloned(), // clone: the history record owns its dataset identifier.
        },
        attribute_type,
    })
}

/// Build the observations and exact instance tombstones produced by a payload mutation.
///
/// A Scope the mutation left behind is recorded too. Clause 4.18 keeps a Scope
/// valid "until the time it has been explicitly removed by an update or delete
/// operation", and a Replace Entity that omits `scope` is such an operation:
/// clause 5.6.18.4 replaces the Entity with the content provided, so the Scope
/// is gone from current state and its temporal interval has to close with it.
/// A mutation that names a different Scope needs no record of its own — the
/// event it writes is itself what ends the interval before it.
///
/// The observation is the payload the write carried, minus its NGSI-LD Null
/// deletion instructions: clause 4.5.0 introduces the sentinel to encode a
/// deletion rather than a value, and what it deleted is already recorded by the
/// tombstones below it. Keeping it would write `urn:ngsi-ld:null` into history
/// as the value of the member the write removed.
///
/// # Errors
///
/// Returns [`CommitError::Invariant`] when either state carries an instance
/// whose `datasetId` cannot be read, since its tombstone would name no
/// instance.
pub fn mutation_history(observed: &Arc<Value>, previous: &Value, current: &Value) -> Result<Vec<HistoryRecord>, CommitError> {
    let removed = removed_attribute_instances(previous, current).map_err(CommitError::invariant)?;
    let mut history = Vec::with_capacity(removed.len() + 2);
    let observation = match without_null_sentinels(observed) {
        Some(stripped) => Arc::new(stripped),
        // clone: history and change-event paths share an unchanged observed payload.
        None => Arc::clone(observed),
    };
    history.push(HistoryRecord::Observations(observation));
    history.extend(removed.into_iter().map(HistoryRecord::Tombstone));
    if scope_was_cleared(previous, current) {
        history.push(HistoryRecord::ScopeRemoval);
    }
    Ok(history)
}

/// Whether the mutation left the Entity with no Scope at all, having had one.
fn scope_was_cleared(previous: &Value, current: &Value) -> bool {
    !extract_scopes(previous).is_empty() && extract_scopes(current).is_empty()
}

/// Build the tombstone every attribute instance of a deleted entity needs.
///
/// Clause 5.6.6 deletes the entity's current state, and clause 4.5.8 has an
/// instance that has ended carry a
/// `deletedAt`. The deletion therefore records one tombstone per instance the
/// entity still held, so temporal history says when each instance ended instead
/// of trailing off with no explanation.
///
/// The order is fixed by the instance identity rather than left to the set that
/// collected it: a tombstone's position in the write is half of the key its
/// stored row deduplicates on, so a replay has to produce the same order.
///
/// # Errors
///
/// Returns [`CommitError::Invariant`] when the entity carries an instance whose
/// `datasetId` cannot be read.
pub fn deletion_history(previous: &Value) -> Result<Vec<HistoryRecord>, CommitError> {
    let mut removed = attribute_instance_families(previous)
        .map_err(CommitError::invariant)?
        .into_iter()
        .map(|(key, attribute_type)| RemovedAttributeInstance { key, attribute_type })
        .collect::<Vec<RemovedAttributeInstance>>();
    removed.sort_by(|left, right| {
        left.key.attr.as_str().cmp(right.key.attr.as_str()).then_with(|| {
            left.key
                .dataset_id
                .as_ref()
                .map(Uri::as_str)
                .cmp(&right.key.dataset_id.as_ref().map(Uri::as_str))
        })
    });
    let mut history: Vec<HistoryRecord> = removed.into_iter().map(HistoryRecord::Tombstone).collect();
    // Clause 4.18 ends a Scope's validity at the "delete operation" as well as
    // at an update, so an Entity that still held a Scope closes its interval
    // here rather than leaving it open past the Entity itself.
    if !extract_scopes(previous).is_empty() {
        history.push(HistoryRecord::ScopeRemoval);
    }
    Ok(history)
}

#[cfg(test)]
mod tests {
    use crate::history_record::{HistoryRecord, deletion_history, mutation_history};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn a_replacement_tombstones_a_dropped_attribute() {
        let previous = json!([{
            "@id": "urn:ngsi-ld:A",
            "https://example.org/speed": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"]}],
            "https://example.org/colour": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"]}]
        }]);
        let replacement = json!([{
            "@id": "urn:ngsi-ld:A",
            "https://example.org/speed": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"]}]
        }]);
        let observed = Arc::new(replacement.clone()); // clone: the test compares the replacement after sharing it.
        let history = mutation_history(&observed, &previous, &replacement).expect("readable datasetIds");
        assert!(history[0].is_observations());
        assert!(history.iter().any(HistoryRecord::is_tombstone));
    }

    #[test]
    fn mutation_history_tombstones_the_exact_removed_dataset_instance() {
        let dataset_a = "urn:ngsi-ld:Dataset:a";
        let dataset_b = "urn:ngsi-ld:Dataset:b";
        let instance = |dataset_id: &str| {
            json!({
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
                "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": dataset_id}]
            })
        };
        let previous = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/speed": [instance(dataset_a), instance(dataset_b)]
        }]);
        let current = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/speed": [instance(dataset_b)]
        }]);
        let observed = Arc::new(json!([{
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "urn:ngsi-ld:null"}],
                "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": dataset_a}]
            }]
        }]));

        let history = mutation_history(&observed, &previous, &current).expect("readable datasetIds");

        pretty_assertions::assert_eq!(history.len(), 2);
        assert!(history[1].is_tombstone());
        let HistoryRecord::Tombstone(removed) = &history[1] else {
            return;
        };
        pretty_assertions::assert_eq!(removed.key.attr.as_str(), "https://example.com/speed");
        pretty_assertions::assert_eq!(removed.key.dataset_id.as_ref().map(Uri::as_str), Some(dataset_a));
    }

    #[test]
    fn deleting_an_entity_tombstones_every_instance_it_still_held_in_a_fixed_order() {
        let previous = json!([{
            "@id": "urn:ngsi-ld:A",
            "https://example.org/speed": [
                {"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:b"}]},
                {"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:a"}]}
            ],
            "https://example.org/colour": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"]}]
        }]);

        let named = deletion_history(&previous)
            .expect("readable datasetIds")
            .into_iter()
            .map(|entry| match entry {
                HistoryRecord::Tombstone(removed) => (removed.key.attr.as_str().to_owned(), removed.key.dataset_id.map(|id| id.as_str().to_owned())),
                HistoryRecord::Observations(_) | HistoryRecord::ScopeRemoval => (String::new(), None),
            })
            .collect::<Vec<_>>();

        pretty_assertions::assert_eq!(
            named,
            vec![
                ("https://example.org/colour".to_owned(), None),
                ("https://example.org/speed".to_owned(), Some("urn:ngsi-ld:Dataset:a".to_owned())),
                ("https://example.org/speed".to_owned(), Some("urn:ngsi-ld:Dataset:b".to_owned())),
            ]
        );
    }

    #[test]
    fn deleting_an_entity_that_held_no_attributes_records_nothing() {
        assert!(deletion_history(&json!([{"@id": "urn:ngsi-ld:A"}])).expect("readable datasetIds").is_empty());
    }
}
