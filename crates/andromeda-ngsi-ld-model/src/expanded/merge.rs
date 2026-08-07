//! Merging expanded JSON-LD fragments into a stored payload.

use crate::expanded::dataset_id::get_instance_dataset_id;
use serde_json::{Map, Value};

/// Merge `patch` into `base` following NGSI-LD expanded form conventions.
///
/// - Arrays are merged element-by-element (recursively).
/// - An object patch on an array-wrapped base merges into the first element.
/// - Object keys starting with `@` are never overwritten.
/// - Object members are replaced (not recursively merged), matching NGSI-LD
///   partial update semantics where attribute instance members are top-level atomic units.
/// - Scalars: replaced entirely.
pub fn merge_json(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Array(base_arr), Value::Array(patch_arr)) => {
            for (i, patch_elem) in patch_arr.iter().enumerate() {
                if i < base_arr.len() {
                    merge_json(&mut base_arr[i], patch_elem);
                } else {
                    // clone: the patch is borrowed and the base takes ownership of the new element.
                    base_arr.push(patch_elem.clone());
                }
            }
        }
        (Value::Array(base_arr), Value::Object(_)) => {
            if let Some(first) = base_arr.first_mut() {
                merge_json(first, patch);
            }
        }
        (Value::Object(base_obj), Value::Object(patch_obj)) => {
            for (key, value) in patch_obj {
                if key.starts_with('@') {
                    continue;
                }
                // clone: both the key and the value are borrowed from the patch and stored owned.
                base_obj.insert(key.clone(), value.clone());
            }
        }
        (base, patch) => {
            // clone: the patch is borrowed and wholly replaces the owned base.
            *base = patch.clone();
        }
    }
}

/// Deep-merge attribute instances into an entity object, matching by datasetId.
pub fn deep_merge_attribute(entity_obj: &mut Map<String, Value>, key: &str, patch_instances: &[Value]) {
    let existing = entity_obj.get_mut(key);
    let Some(existing) = existing else {
        entity_obj.insert(key.to_string(), Value::Array(patch_instances.to_vec()));
        return;
    };
    let Some(existing_arr) = existing.as_array_mut() else {
        entity_obj.insert(key.to_string(), Value::Array(patch_instances.to_vec()));
        return;
    };

    for patch_inst in patch_instances {
        let patch_ds_id = get_instance_dataset_id(patch_inst);

        let matched = existing_arr.iter_mut().find(|inst| get_instance_dataset_id(inst) == patch_ds_id);

        if let Some(existing_inst) = matched {
            // A changed attribute type replaces the whole instance: the members
            // of one family have no meaning under another (clause 5.2).
            let type_changed = {
                let existing_type = existing_inst.get("@type");
                let patch_type = patch_inst.get("@type");
                match (existing_type, patch_type) {
                    (Some(et), Some(pt)) => et != pt,
                    (Some(_), None) | (None, Some(_)) | (None, None) => false,
                }
            };

            if type_changed {
                // clone: the patch is borrowed and the stored instance takes ownership.
                *existing_inst = patch_inst.clone();
            } else if let (Some(base_obj), Some(patch_obj)) = (existing_inst.as_object_mut(), patch_inst.as_object()) {
                for (k, v) in patch_obj {
                    if k.starts_with('@') {
                        continue;
                    }
                    // clone: both the key and the value are borrowed from the patch and stored owned.
                    base_obj.insert(k.clone(), v.clone());
                }
            }
        } else {
            // clone: the patch is borrowed and the instance array takes ownership.
            existing_arr.push(patch_inst.clone());
        }
    }
}

/// Remove instances matching a dataset ID (or the default instance if `None`)
/// from the instances array for an attribute key. Removes the key entirely if
/// no instances remain.
///
/// Returns whether at least one instance was actually removed.
pub(crate) fn remove_instances_by_dataset(entity_obj: &mut Map<String, Value>, key: &str, dataset_id: Option<&str>) -> bool {
    let Some(Value::Array(instances)) = entity_obj.get_mut(key) else {
        return false;
    };

    let before = instances.len();
    match dataset_id {
        Some(ds_id) => {
            instances.retain(|inst| get_instance_dataset_id(inst) != Some(ds_id));
        }
        None => {
            // The default instance is the one carrying no datasetId at all.
            instances.retain(|inst| get_instance_dataset_id(inst).is_some());
        }
    }
    let removed = instances.len() < before;

    if instances.is_empty() {
        entity_obj.remove(key);
    }

    removed
}

#[cfg(test)]
mod tests {
    use crate::expanded::merge::{deep_merge_attribute, merge_json, remove_instances_by_dataset};
    use serde_json::{Map, Value, json};

    /// One attribute instance carrying `value`, optionally tagged with a `datasetId`.
    fn instance(value: i64, dataset: Option<&str>) -> Value {
        let mut node = json!({"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]});
        if let Some(dataset) = dataset {
            node["https://uri.etsi.org/ngsi-ld/datasetId"] = json!([{"@id": dataset}]);
        }
        node
    }

    /// The entity object the `deep_merge_attribute` cases operate on.
    fn entity_with(instances: Vec<Value>) -> Map<String, Value> {
        let mut object = Map::new();
        object.insert("https://example.com/speed".to_string(), Value::Array(instances));
        object
    }

    #[test]
    fn merging_two_arrays_merges_them_element_by_element() {
        let mut base = json!([{"a": 1}, {"b": 2}]);
        merge_json(&mut base, &json!([{"a": 9}]));
        pretty_assertions::assert_eq!(base, json!([{"a": 9}, {"b": 2}]));
    }

    #[test]
    fn merging_a_longer_array_appends_the_surplus_elements() {
        let mut base = json!([{"a": 1}]);
        merge_json(&mut base, &json!([{"a": 1}, {"b": 2}]));
        pretty_assertions::assert_eq!(base, json!([{"a": 1}, {"b": 2}]));
    }

    #[test]
    fn merging_an_object_into_an_array_merges_into_its_first_element() {
        let mut base = json!([{"a": 1}, {"b": 2}]);
        merge_json(&mut base, &json!({"a": 9}));
        pretty_assertions::assert_eq!(base, json!([{"a": 9}, {"b": 2}]));
    }

    #[test]
    fn merging_an_object_into_an_empty_array_changes_nothing() {
        let mut base = json!([]);
        merge_json(&mut base, &json!({"a": 9}));
        pretty_assertions::assert_eq!(base, json!([]));
    }

    #[test]
    fn merging_two_objects_replaces_members_but_never_a_keyword() {
        let mut base = json!({"@id": "urn:ngsi-ld:Vehicle:1", "a": {"nested": 1}});
        merge_json(&mut base, &json!({"@id": "urn:ngsi-ld:Vehicle:2", "a": {"other": 2}}));
        pretty_assertions::assert_eq!(base, json!({"@id": "urn:ngsi-ld:Vehicle:1", "a": {"other": 2}}));
    }

    #[test]
    fn merging_onto_a_scalar_replaces_it_wholesale() {
        let mut base = json!(1);
        merge_json(&mut base, &json!({"a": 2}));
        pretty_assertions::assert_eq!(base, json!({"a": 2}));
    }

    #[test]
    fn deep_merging_an_absent_attribute_inserts_the_patch_instances() {
        let mut object = Map::new();
        deep_merge_attribute(&mut object, "https://example.com/speed", &[instance(10, None)]);
        pretty_assertions::assert_eq!(object["https://example.com/speed"], json!([instance(10, None)]));
    }

    #[test]
    fn deep_merging_over_a_non_array_member_replaces_it() {
        let mut object = Map::new();
        object.insert("https://example.com/speed".to_string(), json!("not an instance array"));
        deep_merge_attribute(&mut object, "https://example.com/speed", &[instance(10, None)]);
        pretty_assertions::assert_eq!(object["https://example.com/speed"], json!([instance(10, None)]));
    }

    #[test]
    fn deep_merging_a_matching_dataset_id_merges_the_instance_members() {
        let mut object = entity_with(vec![instance(10, Some("urn:ngsi-ld:Dataset:A"))]);
        let mut patch = instance(20, Some("urn:ngsi-ld:Dataset:A"));
        patch["https://uri.etsi.org/ngsi-ld/unitCode"] = json!([{"@value": "KMH"}]);
        deep_merge_attribute(&mut object, "https://example.com/speed", &[patch]);

        let instances = object["https://example.com/speed"].as_array().expect("instances");
        pretty_assertions::assert_eq!(instances.len(), 1);
        pretty_assertions::assert_eq!(instances[0]["https://uri.etsi.org/ngsi-ld/hasValue"], json!([{"@value": 20}]));
        pretty_assertions::assert_eq!(instances[0]["https://uri.etsi.org/ngsi-ld/unitCode"], json!([{"@value": "KMH"}]));
    }

    #[test]
    fn deep_merging_a_changed_attribute_type_replaces_the_whole_instance() {
        let mut existing = instance(10, None);
        existing["@type"] = json!(["https://uri.etsi.org/ngsi-ld/Property"]);
        existing["https://uri.etsi.org/ngsi-ld/unitCode"] = json!([{"@value": "KMH"}]);
        let mut object = entity_with(vec![existing]);

        let mut patch = json!({"https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Vehicle:2"}]});
        patch["@type"] = json!(["https://uri.etsi.org/ngsi-ld/Relationship"]);
        deep_merge_attribute(&mut object, "https://example.com/speed", &[patch.clone()]); // clone: the patch is compared below

        pretty_assertions::assert_eq!(object["https://example.com/speed"], json!([patch]));
    }

    #[test]
    fn deep_merging_an_unmatched_dataset_id_appends_a_new_instance() {
        let mut object = entity_with(vec![instance(10, Some("urn:ngsi-ld:Dataset:A"))]);
        deep_merge_attribute(&mut object, "https://example.com/speed", &[instance(20, Some("urn:ngsi-ld:Dataset:B"))]);
        pretty_assertions::assert_eq!(object["https://example.com/speed"].as_array().expect("instances").len(), 2);
    }

    #[test]
    fn removing_a_named_dataset_id_drops_only_that_instance() {
        // Clause 4.5.5: `datasetId` is what separates the instances of a
        // multi-attribute, so a delete naming one must leave the others standing.
        let mut object = entity_with(vec![
            instance(10, Some("urn:ngsi-ld:Dataset:A")),
            instance(20, Some("urn:ngsi-ld:Dataset:B")),
            instance(30, None),
        ]);
        assert!(remove_instances_by_dataset(
            &mut object,
            "https://example.com/speed",
            Some("urn:ngsi-ld:Dataset:A")
        ));
        let instances = object["https://example.com/speed"].as_array().expect("instances");
        pretty_assertions::assert_eq!(instances.len(), 2);
        assert!(
            instances
                .iter()
                .all(|inst| inst["https://uri.etsi.org/ngsi-ld/hasValue"][0]["@value"] != json!(10))
        );
    }

    #[test]
    fn removing_the_default_instance_drops_the_one_carrying_no_dataset_id() {
        let mut object = entity_with(vec![instance(10, Some("urn:ngsi-ld:Dataset:A")), instance(30, None)]);
        assert!(remove_instances_by_dataset(&mut object, "https://example.com/speed", None));
        let instances = object["https://example.com/speed"].as_array().expect("instances");
        pretty_assertions::assert_eq!(instances.len(), 1);
        pretty_assertions::assert_eq!(instances[0]["https://uri.etsi.org/ngsi-ld/hasValue"], json!([{"@value": 10}]));
    }

    #[test]
    fn removing_an_unmatched_dataset_id_reports_that_nothing_was_removed() {
        let mut object = entity_with(vec![instance(10, Some("urn:ngsi-ld:Dataset:A"))]);
        assert!(!remove_instances_by_dataset(
            &mut object,
            "https://example.com/speed",
            Some("urn:ngsi-ld:Dataset:Z")
        ));
        pretty_assertions::assert_eq!(object["https://example.com/speed"].as_array().expect("instances").len(), 1);
    }

    #[test]
    fn removing_the_last_instance_drops_the_attribute_member_itself() {
        let mut object = entity_with(vec![instance(10, Some("urn:ngsi-ld:Dataset:A"))]);
        assert!(remove_instances_by_dataset(
            &mut object,
            "https://example.com/speed",
            Some("urn:ngsi-ld:Dataset:A")
        ));
        assert!(object.get("https://example.com/speed").is_none());
    }

    #[test]
    fn removing_from_an_absent_attribute_reports_that_nothing_was_removed() {
        let mut object = Map::new();
        assert!(!remove_instances_by_dataset(&mut object, "https://example.com/speed", None));
    }
}
