//! Precise current-state changes of NGSI-LD Attribute instances.

use crate::{
    expanded::{
        dataset_id::{InstanceDatasetIdError, get_instance_dataset_id},
        instance_key::{AttributeInstanceKey, is_system_attribute},
        node::expanded_object_ref,
    },
    notification::NotificationTrigger,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use serde_json::Value;
use std::{cmp::Ordering, collections::HashMap};

/// The precise state transition of one Attribute instance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttributeChangeKind {
    /// The instance was absent and is now present.
    Created,
    /// The instance remains present and its JSON-LD node changed.
    Updated,
    /// The instance was present and is now absent.
    Deleted,
}

impl AttributeChangeKind {
    /// Clause-5.2.12 notification trigger corresponding to this transition.
    #[must_use]
    pub const fn notification_trigger(self) -> NotificationTrigger {
        match self {
            AttributeChangeKind::Created => NotificationTrigger::AttributeCreated,
            AttributeChangeKind::Updated => NotificationTrigger::AttributeUpdated,
            AttributeChangeKind::Deleted => NotificationTrigger::AttributeDeleted,
        }
    }
}

/// One classified Attribute-instance transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttributeChange {
    /// Attribute name and dataset discriminator of the changed instance.
    pub key: AttributeInstanceKey,
    /// Transition that occurred.
    pub kind: AttributeChangeKind,
}

/// Compare two expanded Entity states at Attribute-instance granularity.
///
/// Clause 4.5.5 makes `(attribute name, datasetId)` the instance identity. A
/// node present only before is deleted, one present only after is created, and
/// a retained node whose expanded representation differs is updated. Entity
/// keywords, system attributes, and `scope` are not Attributes and are
/// excluded.
///
/// # Errors
///
/// Returns [`InstanceDatasetIdError`] when either state carries an instance
/// whose `datasetId` is not an identifier, since its identity cannot be told.
pub fn classify_attribute_changes(previous: &Value, current: &Value) -> Result<Vec<AttributeChange>, InstanceDatasetIdError> {
    let previous = attribute_instances(previous)?;
    let current = attribute_instances(current)?;
    let mut changes = Vec::with_capacity(previous.len().saturating_add(current.len()));

    for (key, previous_instance) in &previous {
        match current.get(key) {
            None => changes.push(AttributeChange {
                key: key.clone(),
                kind: AttributeChangeKind::Deleted,
            }),
            Some(current_instance) if instance_data_differs(current_instance, previous_instance) => changes.push(AttributeChange {
                key: key.clone(),
                kind: AttributeChangeKind::Updated,
            }),
            Some(_) => {}
        }
    }
    for key in current.keys().filter(|key| !previous.contains_key(*key)) {
        changes.push(AttributeChange {
            key: key.clone(),
            kind: AttributeChangeKind::Created,
        });
    }
    changes.sort_by(compare_changes);
    Ok(changes)
}

/// Whether two instances of the same `(attribute, datasetId)` differ in their
/// data, ignoring the clause 4.8 system times `createdAt`/`modifiedAt`.
///
/// A write stamps `modifiedAt` and preserves or sets `createdAt` on every
/// instance it touches, and both members appear in the stored payload; neither
/// is a change a Context Producer made, so a change classification that read
/// them would raise an `attributeUpdated` for an instance whose data is
/// unchanged -- and would raise one for every attribute of an entity stored
/// before the broker began stamping, whose first update introduces the members.
fn instance_data_differs(current: &Value, previous: &Value) -> bool {
    let system_time = |key: &str| key == properties::CREATED_AT.as_str() || key == properties::MODIFIED_AT.as_str();
    match (current.as_object(), previous.as_object()) {
        (Some(current), Some(previous)) => {
            let significant = |members: &serde_json::Map<String, Value>| members.keys().filter(|key| !system_time(key)).count();
            significant(current) != significant(previous)
                || current
                    .iter()
                    .filter(|(key, _)| !system_time(key))
                    .any(|(key, value)| previous.get(key) != Some(value))
        }
        (Some(_) | None, _) => current != previous,
    }
}

/// Index one expanded entity's attribute instances by their dataset-partitioned key.
fn attribute_instances(expanded: &Value) -> Result<HashMap<AttributeInstanceKey, &Value>, InstanceDatasetIdError> {
    let mut instances_by_key = HashMap::new();
    let Some(entity) = expanded_object_ref(expanded) else {
        return Ok(instances_by_key);
    };
    for (raw_name, value) in entity {
        if raw_name.starts_with('@') {
            continue;
        }
        let Ok(attr) = raw_name.parse::<AttributeName>() else {
            continue;
        };
        if is_system_attribute(&attr) || attr.as_str() == properties::SCOPE.as_str() {
            continue;
        }
        let Some(instances) = value.as_array() else {
            continue;
        };
        for instance in instances {
            instances_by_key.insert(
                AttributeInstanceKey {
                    attr: attr.clone(), // clone: every dataset-partitioned instance owns its key.
                    dataset_id: get_instance_dataset_id(instance)?,
                },
                instance,
            );
        }
    }
    Ok(instances_by_key)
}

/// Order two changes by attribute name, then by `datasetId`, for a stable result.
fn compare_changes(left: &AttributeChange, right: &AttributeChange) -> Ordering {
    left.key
        .attr
        .as_str()
        .cmp(right.key.attr.as_str())
        .then_with(|| dataset_str(&left.key).cmp(dataset_str(&right.key)))
}

/// The `datasetId` of one instance key as text, empty for the default instance.
fn dataset_str(key: &AttributeInstanceKey) -> &str {
    key.dataset_id.as_ref().map_or("", |dataset_id| dataset_id.as_str())
}

#[cfg(test)]
mod tests {
    use crate::expanded::attribute_change::{AttributeChange, AttributeChangeKind, classify_attribute_changes};
    use serde_json::{Value, json};

    fn property(value: i64, dataset_id: Option<&str>) -> Value {
        let mut property = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]
        });
        if let (Some(dataset_id), Some(object)) = (dataset_id, property.as_object_mut()) {
            object.insert("https://uri.etsi.org/ngsi-ld/datasetId".to_owned(), json!([{"@id": dataset_id}]));
        }
        property
    }

    #[test]
    fn diff_classifies_created_updated_deleted_and_unchanged_instances() {
        let previous = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/speed": [property(1, None), property(2, Some("urn:dataset:old"))],
            "https://example.com/unchanged": [property(3, None)]
        }]);
        let current = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "https://example.com/speed": [property(4, None), property(5, Some("urn:dataset:new"))],
            "https://example.com/unchanged": [property(3, None)]
        }]);
        let changes = classify_attribute_changes(&previous, &current).expect("readable datasetIds");
        pretty_assertions::assert_eq!(changes.len(), 3);
        assert!(
            changes
                .iter()
                .any(|change| change.kind == AttributeChangeKind::Updated && change.key.dataset_id.is_none())
        );
        assert!(changes.iter().any(|change| change.kind == AttributeChangeKind::Deleted));
        assert!(changes.iter().any(|change| change.kind == AttributeChangeKind::Created));
    }

    #[test]
    fn diff_excludes_entity_members_and_system_attributes() {
        let previous = json!([{"@id": "urn:ngsi-ld:Vehicle:1", "@type": ["https://example.com/A"]}]);
        let current = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.com/B"],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/A"}],
            "https://uri.etsi.org/ngsi-ld/modifiedAt": [{"@value": "2026-01-01T00:00:00Z"}]
        }]);
        pretty_assertions::assert_eq!(classify_attribute_changes(&previous, &current), Ok(Vec::<AttributeChange>::new()));
    }
}
