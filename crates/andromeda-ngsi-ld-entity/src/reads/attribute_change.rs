//! The precise current-state change of each attribute instance between two
//! states (clause 4.5.5 identity, clause 5.2.12 triggers).

use crate::{entity_state::EntityState, instance::Instance, reads::attribute_instance_key::AttributeInstanceKey};
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::HashMap};

/// The state transition of one attribute instance.
///
/// Not a specification token: the kebab-case spelling is the one Andromeda
/// persists a deferred change under.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AttributeChangeKind {
    /// The instance was absent and is now present.
    Created,
    /// The instance remains present and its content changed.
    Updated,
    /// The instance was present and is now absent.
    Deleted,
}

/// One classified attribute-instance transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttributeChange {
    /// Attribute name and `datasetId` of the changed instance.
    pub key: AttributeInstanceKey,
    /// The transition.
    pub kind: AttributeChangeKind,
}

/// Compare two states at attribute-instance granularity.
///
/// Clause 4.5.5 makes `(attribute name, datasetId)` the instance identity. An
/// instance present only before is deleted, one present only after is
/// created, and a retained one whose content differs is updated. The clause
/// 4.8 system times are not content: a write stamps them on every instance it
/// touches. `scope` and the entity expiry are not attributes. `previous` is
/// `None` for a creation.
#[must_use]
pub fn classify_attribute_changes(previous: Option<&EntityState>, current: &EntityState) -> Vec<AttributeChange> {
    let previous = previous.map(instances_by_key).unwrap_or_default();
    let current = instances_by_key(current);
    let mut changes = Vec::with_capacity(previous.len().saturating_add(current.len()));
    for (key, previous_instance) in &previous {
        match current.get(key) {
            None => changes.push(change(key, AttributeChangeKind::Deleted)),
            Some(current_instance) if !current_instance.same_content(previous_instance) => changes.push(change(key, AttributeChangeKind::Updated)),
            Some(_) => {}
        }
    }
    changes.extend(
        current
            .keys()
            .filter(|key| !previous.contains_key(*key))
            .map(|key| change(key, AttributeChangeKind::Created)),
    );
    changes.sort_by(compare_changes);
    changes
}

/// Every instance an entity held, deleted, in the order
/// [`classify_attribute_changes`] reports changes: the change a deletion makes.
#[must_use]
pub fn deleted_instances(keys: impl IntoIterator<Item = AttributeInstanceKey>) -> Vec<AttributeChange> {
    let mut changes: Vec<AttributeChange> = keys
        .into_iter()
        .map(|key| AttributeChange {
            key,
            kind: AttributeChangeKind::Deleted,
        })
        .collect();
    changes.sort_by(compare_changes);
    changes.dedup_by(|left, right| compare_changes(left, right) == Ordering::Equal);
    changes
}

/// One change of `key`.
fn change(key: &AttributeInstanceKey, kind: AttributeChangeKind) -> AttributeChange {
    AttributeChange { key: key.clone(), kind }
}

/// Every instance of `state` by its identity.
fn instances_by_key(state: &EntityState) -> HashMap<AttributeInstanceKey, &Instance> {
    state
        .attributes()
        .iter()
        .flat_map(|(name, instances)| {
            instances.iter().map(move |(dataset_id, instance)| {
                (
                    AttributeInstanceKey {
                        attr: name.clone(),
                        dataset_id: dataset_id.cloned(),
                    },
                    instance,
                )
            })
        })
        .collect()
}

/// Changes ordered by attribute name, then `datasetId`, the default instance
/// first.
fn compare_changes(left: &AttributeChange, right: &AttributeChange) -> Ordering {
    left.key
        .attr
        .cmp(&right.key.attr)
        .then_with(|| dataset_text(&left.key).cmp(dataset_text(&right.key)))
}

/// The `datasetId` text of a key, empty for the default instance.
fn dataset_text(key: &AttributeInstanceKey) -> &str {
    key.dataset_id.as_ref().map_or("", |dataset_id| dataset_id.as_str())
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_state::EntityState,
        reads::{
            attribute_change::{AttributeChange, AttributeChangeKind, classify_attribute_changes, deleted_instances},
            attribute_instance_key::AttributeInstanceKey,
        },
    };
    use serde_json::{Value, json};

    const PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/Property";

    fn state(members: &Value) -> EntityState {
        let mut node = json!({"@id": "urn:ngsi-ld:E:1", "@type": ["https://ex.org/T"]});
        if let (Some(node), Some(members)) = (node.as_object_mut(), members.as_object()) {
            node.extend(members.iter().map(|(key, value)| (key.clone(), value.clone())));
        }
        EntityState::from_slice(&serde_json::to_vec(&json!([node])).expect("JSON")).expect("a state")
    }

    fn instance(value: i64, dataset: Option<&str>, modified: &str) -> Value {
        let mut instance = json!({
            "@type": [PROPERTY],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}],
            "https://uri.etsi.org/ngsi-ld/modifiedAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": modified}]
        });
        if let Some(dataset) = dataset {
            instance["https://uri.etsi.org/ngsi-ld/datasetId"] = json!([{"@id": dataset}]);
        }
        instance
    }

    fn key(attr: &str, dataset: Option<&str>) -> AttributeInstanceKey {
        AttributeInstanceKey {
            attr: attr.parse().expect("a name"),
            dataset_id: dataset.map(|dataset| dataset.parse().expect("an identifier")),
        }
    }

    #[test]
    fn diff_classifies_created_updated_deleted_and_unchanged_instances() {
        let previous = state(&json!({
            "https://ex.org/a": [instance(1, None, "2026-01-01T00:00:00.000000Z")],
            "https://ex.org/b": [instance(2, None, "2026-01-01T00:00:00.000000Z"), instance(3, Some("urn:ngsi-ld:Dataset:x"), "2026-01-01T00:00:00.000000Z")],
            "https://ex.org/c": [instance(4, None, "2026-01-01T00:00:00.000000Z")]
        }));
        let current = state(&json!({
            "https://ex.org/a": [instance(1, None, "2026-02-01T00:00:00.000000Z")],
            "https://ex.org/b": [instance(9, None, "2026-02-01T00:00:00.000000Z")],
            "https://ex.org/d": [instance(5, None, "2026-02-01T00:00:00.000000Z")]
        }));
        pretty_assertions::assert_eq!(
            classify_attribute_changes(Some(&previous), &current),
            vec![
                AttributeChange {
                    key: key("https://ex.org/b", None),
                    kind: AttributeChangeKind::Updated
                },
                AttributeChange {
                    key: key("https://ex.org/b", Some("urn:ngsi-ld:Dataset:x")),
                    kind: AttributeChangeKind::Deleted
                },
                AttributeChange {
                    key: key("https://ex.org/c", None),
                    kind: AttributeChangeKind::Deleted
                },
                AttributeChange {
                    key: key("https://ex.org/d", None),
                    kind: AttributeChangeKind::Created
                },
            ]
        );
    }

    #[test]
    fn diff_excludes_the_scope_and_the_system_times() {
        let previous =
            state(&json!({"https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/a"}], "https://ex.org/a": [instance(1, None, "2026-01-01T00:00:00.000000Z")]}));
        let current =
            state(&json!({"https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/b"}], "https://ex.org/a": [instance(1, None, "2026-03-01T00:00:00.000000Z")]}));
        pretty_assertions::assert_eq!(classify_attribute_changes(Some(&previous), &current), Vec::new());
    }

    #[test]
    fn a_creation_classifies_every_instance_as_created() {
        let current = state(&json!({"https://ex.org/a": [instance(1, None, "2026-01-01T00:00:00.000000Z")]}));
        pretty_assertions::assert_eq!(
            classify_attribute_changes(None, &current),
            vec![AttributeChange {
                key: key("https://ex.org/a", None),
                kind: AttributeChangeKind::Created
            }]
        );
    }

    #[test]
    fn deleted_instances_lists_each_identity_once_in_classification_order() {
        let changes = deleted_instances([
            key("https://ex.org/b", None),
            key("https://ex.org/a", Some("urn:ngsi-ld:Dataset:x")),
            key("https://ex.org/b", None),
        ]);
        pretty_assertions::assert_eq!(
            changes.iter().map(|change| change.key.clone()).collect::<Vec<_>>(),
            vec![key("https://ex.org/a", Some("urn:ngsi-ld:Dataset:x")), key("https://ex.org/b", None)]
        );
        assert!(changes.iter().all(|change| change.kind == AttributeChangeKind::Deleted));
    }

    #[test]
    fn deleted_instances_agrees_with_classifying_against_no_instances() {
        let previous = state(&json!({"https://ex.org/a": [instance(1, None, "2026-01-01T00:00:00.000000Z")]}));
        let emptied = state(&json!({}));
        pretty_assertions::assert_eq!(
            classify_attribute_changes(Some(&previous), &emptied),
            deleted_instances([key("https://ex.org/a", None)])
        );
    }

    #[test]
    fn deleted_instances_of_no_identity_is_no_change() {
        pretty_assertions::assert_eq!(deleted_instances([]), Vec::new());
    }

    #[test]
    fn the_change_kind_keeps_its_persisted_spelling() {
        pretty_assertions::assert_eq!(serde_json::to_value(AttributeChangeKind::Updated).ok(), Some(json!("updated")));
    }
}
