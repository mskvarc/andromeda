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
            instances.retain(|inst| get_instance_dataset_id(inst).as_deref() != Some(ds_id));
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
