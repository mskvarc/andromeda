//! The instances a replacement state no longer holds (clauses 4.5.7, 4.5.8).
//!
//! A removed instance keeps its family: its temporal tombstone renders the
//! family's own deletion member, and the instance itself is gone from the
//! replacement.

use crate::{entity_state::EntityState, reads::attribute_instance_key::AttributeInstanceKey};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;

/// One instance a write removed.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RemovedAttributeInstance {
    /// The removed instance's identity.
    pub key: AttributeInstanceKey,
    /// The family the removed instance declared.
    pub attribute_type: AttributeType,
}

/// The instances `previous` holds and `replacement` does not, by identity
/// (attribute and `datasetId`; a family change is an update, not a removal),
/// in `previous`'s stored order.
#[must_use]
pub fn removed_attribute_instances(previous: &EntityState, replacement: &EntityState) -> Vec<RemovedAttributeInstance> {
    previous
        .attributes()
        .iter()
        .flat_map(|(name, instances)| {
            let kept = replacement.attributes().get(name);
            instances
                .iter()
                .filter(move |(dataset_id, _)| kept.is_none_or(|kept| !kept.contains(*dataset_id)))
                .map(move |(dataset_id, instance)| RemovedAttributeInstance {
                    key: AttributeInstanceKey {
                        attr: name.clone(),
                        dataset_id: dataset_id.cloned(),
                    },
                    attribute_type: instance.family(),
                })
        })
        .collect()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        entity_state::EntityState,
        reads::{
            attribute_instance_key::AttributeInstanceKey,
            removed_instance::{RemovedAttributeInstance, removed_attribute_instances},
        },
    };
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use serde_json::{Value, json};

    fn state(members: &Value) -> EntityState {
        let mut node = json!({"@id": "urn:ngsi-ld:E:1", "@type": ["https://ex.org/T"]});
        if let (Some(node), Some(members)) = (node.as_object_mut(), members.as_object()) {
            node.extend(members.iter().map(|(key, value)| (key.clone(), value.clone())));
        }
        EntityState::from_slice(&serde_json::to_vec(&json!([node])).expect("JSON")).expect("a state")
    }

    fn property(dataset: Option<&str>) -> Value {
        let mut instance = json!({"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]});
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
    fn removed_instances_reports_attributes_a_replacement_drops() {
        let previous = state(&json!({"https://ex.org/a": [property(None)], "https://ex.org/b": [property(None)]}));
        let replacement = state(&json!({"https://ex.org/a": [property(None)]}));
        pretty_assertions::assert_eq!(
            removed_attribute_instances(&previous, &replacement),
            vec![RemovedAttributeInstance {
                key: key("https://ex.org/b", None),
                attribute_type: AttributeType::Property
            }]
        );
    }

    #[test]
    fn removed_instances_is_empty_when_nothing_was_dropped() {
        let previous = state(&json!({"https://ex.org/a": [property(None)]}));
        pretty_assertions::assert_eq!(removed_attribute_instances(&previous, &previous), Vec::new());
    }

    #[test]
    fn removed_instances_distinguishes_dataset_partitioned_instances() {
        let previous = state(&json!({"https://ex.org/a": [property(None), property(Some("urn:ngsi-ld:Dataset:x"))]}));
        let replacement = state(&json!({"https://ex.org/a": [property(None)]}));
        pretty_assertions::assert_eq!(
            removed_attribute_instances(&previous, &replacement),
            vec![RemovedAttributeInstance {
                key: key("https://ex.org/a", Some("urn:ngsi-ld:Dataset:x")),
                attribute_type: AttributeType::Property
            }]
        );
    }

    #[test]
    fn removed_instances_accepts_non_urn_dataset_ids() {
        let previous = state(&json!({"https://ex.org/a": [property(Some("https://ex.org/dataset/1"))]}));
        let replacement = state(&json!({}));
        pretty_assertions::assert_eq!(removed_attribute_instances(&previous, &replacement).len(), 1);
    }

    #[test]
    fn a_removed_instance_carries_the_family_it_declared() {
        let previous = state(
            &json!({"https://ex.org/rel": [{"@type": ["https://uri.etsi.org/ngsi-ld/Relationship"], "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:X:1"}]}]}),
        );
        let replacement = state(&json!({}));
        pretty_assertions::assert_eq!(
            removed_attribute_instances(&previous, &replacement)
                .first()
                .map(|removed| removed.attribute_type),
            Some(AttributeType::Relationship)
        );
    }
}
