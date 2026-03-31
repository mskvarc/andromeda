//! Operations on expanded JSON-LD entities following NGSI-LD semantics (ETSI GS CIM 009 clause 4.6.2).
//!
//! All functions in this module operate on the **expanded** form of JSON-LD
//! (full IRIs, array-wrapped values) as stored in the database.  They are pure
//! functions with no I/O — the caller is responsible for loading and persisting
//! the entity.

use crate::domain::iri;
use serde_json::{Map, Value};

// ---------------------------------------------------------------------------
// Null sentinel detection
// ---------------------------------------------------------------------------

/// Returns `true` if the expanded attribute instance is a deletion sentinel
/// (`urn:ngsi-ld:null` or its expanded IRI form in hasValue, hasObject, or
/// hasLanguageMap).
pub fn instance_is_null_sentinel(instance: &Value) -> bool {
    let Some(obj) = instance.as_object() else {
        return false;
    };

    let check_value_null = |key: &str, value_key: &str| -> bool {
        obj.get(key)
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_object())
            .and_then(|o| o.get(value_key))
            .and_then(|v| v.as_str())
            .is_some_and(|s| s == iri::NULL_URI.as_str() || s == iri::NULL_IRI.as_str())
    };

    check_value_null(iri::HAS_VALUE.as_str(), "@value")
        || check_value_null(iri::HAS_OBJECT.as_str(), "@id")
        || check_value_null(iri::HAS_LANGUAGE_MAP.as_str(), "@value")
}

// ---------------------------------------------------------------------------
// Dataset ID extraction
// ---------------------------------------------------------------------------

/// Extract the `datasetId` (`@id` value) from an expanded attribute instance.
pub fn get_instance_dataset_id(instance: &Value) -> Option<String> {
    instance
        .get(iri::DATASET_ID.as_str())
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_object())
        .and_then(|o| o.get("@id"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

// ---------------------------------------------------------------------------
// Expanded object accessors
// ---------------------------------------------------------------------------

/// Get a mutable reference to the first object in an expanded JSON-LD value.
///
/// Expanded JSON-LD wraps the top-level node in an array: `[{ ... }]`.  This
/// helper handles both the array wrapper and bare object forms.
pub fn expanded_object_mut(entity: &mut Value) -> Option<&mut Map<String, Value>> {
    match entity {
        Value::Array(arr) => arr.first_mut().and_then(Value::as_object_mut),
        Value::Object(obj) => Some(obj),
        _ => None,
    }
}

/// Get a shared reference to the first object in an expanded JSON-LD value.
pub fn expanded_object_ref(value: &Value) -> Option<&Map<String, Value>> {
    match value {
        Value::Array(arr) => arr.first().and_then(Value::as_object),
        Value::Object(obj) => Some(obj),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Type extraction
// ---------------------------------------------------------------------------

/// Extract `@type` IRIs from an expanded entity payload.
pub fn extract_types(expanded: &Value) -> Vec<String> {
    expanded_object_ref(expanded)
        .and_then(|obj| obj.get("@type"))
        .and_then(|t| t.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Scope extraction
// ---------------------------------------------------------------------------

/// Extract scope strings from an expanded entity payload.
pub fn extract_scopes(expanded: &Value) -> Vec<String> {
    let Some(obj) = expanded_object_ref(expanded) else {
        return Vec::new();
    };
    let Some(scope_value) = obj.get(iri::SCOPE.as_str()) else {
        return Vec::new();
    };
    match scope_value {
        Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_object().and_then(|obj| obj.get("@value")).and_then(|v| v.as_str()).map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Location GeoJSON extraction
// ---------------------------------------------------------------------------

const LOCATION_IRI: &str = "https://uri.etsi.org/ngsi-ld/location";
const GEOJSON_VOCAB: &str = "https://purl.org/geojson/vocab#";
const GEOJSON_COORDINATES: &str = "https://purl.org/geojson/vocab#coordinates";

/// Extract the GeoJSON string from the entity's `location` GeoProperty
/// (expanded form).  Returns `None` if no location attribute is present.
pub fn extract_location_geojson(expanded: &Value) -> Option<String> {
    let obj = expanded_object_ref(expanded)?;
    let location = obj.get(LOCATION_IRI)?;

    // Use the last instance (most recent for temporal entities with multiple location instances)
    let geo_node = location
        .as_array()
        .and_then(|arr| arr.last())
        .and_then(|inst| inst.get(iri::HAS_VALUE.as_str()))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())?;

    // Case 1: literal @value (string or object)
    if let Some(val) = geo_node.get("@value") {
        return match val {
            Value::String(s) => Some(s.clone()),
            val if val.is_object() => Some(val.to_string()),
            _ => None,
        };
    }

    // Case 2: expanded JSON-LD GeoJSON node (purl.org/geojson/vocab# IRIs)
    if geo_node
        .as_object()
        .is_some_and(|o| o.contains_key("@type") || o.contains_key(GEOJSON_COORDINATES))
    {
        return Some(reconstruct_geojson(geo_node));
    }

    None
}

/// Reconstruct compact GeoJSON from an expanded JSON-LD GeoJSON node.
pub fn reconstruct_geojson(node: &Value) -> String {
    let Some(obj) = node.as_object() else {
        return node.to_string();
    };

    let geo_type = obj
        .get("@type")
        .and_then(|t| t.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .and_then(|s| s.strip_prefix(GEOJSON_VOCAB))
        .unwrap_or("Point");

    let coordinates = obj
        .get(GEOJSON_COORDINATES)
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("@list").or(Some(v)))
        .map(extract_coordinates)
        .unwrap_or(Value::Array(vec![]));

    let result = serde_json::json!({
        "type": geo_type,
        "coordinates": coordinates
    });
    result.to_string()
}

/// Recursively extract coordinate values from expanded JSON-LD @list structures.
pub fn extract_coordinates(value: &Value) -> Value {
    match value {
        Value::Array(arr) => {
            let items: Vec<Value> = arr.iter().map(extract_coordinates).collect();
            Value::Array(items)
        }
        Value::Object(obj) => {
            if let Some(val) = obj.get("@value") {
                val.clone()
            } else if let Some(list) = obj.get("@list") {
                extract_coordinates(list)
            } else {
                value.clone()
            }
        }
        _ => value.clone(),
    }
}

// ---------------------------------------------------------------------------
// JSON merge
// ---------------------------------------------------------------------------

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
                base_obj.insert(key.clone(), value.clone());
            }
        }
        (base, patch) => {
            *base = patch.clone();
        }
    }
}

// ---------------------------------------------------------------------------
// Attribute-level operations
// ---------------------------------------------------------------------------

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
            // If the attribute type changed, replace the entire instance
            let type_changed = {
                let existing_type = existing_inst.get("@type");
                let patch_type = patch_inst.get("@type");
                match (existing_type, patch_type) {
                    (Some(et), Some(pt)) => et != pt,
                    _ => false,
                }
            };

            if type_changed {
                *existing_inst = patch_inst.clone();
            } else if let (Some(base_obj), Some(patch_obj)) = (existing_inst.as_object_mut(), patch_inst.as_object()) {
                for (k, v) in patch_obj {
                    if k.starts_with('@') {
                        continue;
                    }
                    base_obj.insert(k.clone(), v.clone());
                }
            }
        } else {
            existing_arr.push(patch_inst.clone());
        }
    }
}

/// Remove instances matching a dataset ID (or the default instance if `None`)
/// from the instances array for an attribute key.  Removes the key entirely if
/// no instances remain.
/// Returns `true` if at least one instance was actually removed.
fn remove_instances_by_dataset(entity_obj: &mut Map<String, Value>, key: &str, dataset_id: Option<&str>) -> bool {
    let Some(Value::Array(instances)) = entity_obj.get_mut(key) else {
        return false;
    };

    let before = instances.len();
    match dataset_id {
        Some(ds_id) => {
            instances.retain(|inst| get_instance_dataset_id(inst).as_deref() != Some(ds_id));
        }
        None => {
            // Remove the default instance (the one without datasetId)
            instances.retain(|inst| get_instance_dataset_id(inst).is_some());
        }
    }
    let removed = instances.len() < before;

    if instances.is_empty() {
        entity_obj.remove(key);
    }

    removed
}

// ---------------------------------------------------------------------------
// Entity-level mutation operations
// ---------------------------------------------------------------------------

/// NGSI-LD Merge Patch (§5.6.1): deep-merge `patch` into `entity`, handling
/// null sentinels for attribute deletion.
pub fn merge_entity(entity: &mut Value, patch: &Value) -> Result<(), String> {
    let entity_obj = expanded_object_mut(entity).ok_or("Entity data is not a valid JSON-LD expanded value")?;
    let patch_obj = expanded_object_ref(patch).ok_or("Patch data is not a valid JSON-LD expanded value")?;

    for (key, value) in patch_obj {
        // Merge @type arrays (union, deduplicate) so entity type additions work
        if key == "@type" {
            if let (Some(entity_types), Some(patch_types)) = (entity_obj.get_mut("@type").and_then(Value::as_array_mut), value.as_array()) {
                for t in patch_types {
                    if !entity_types.contains(t) {
                        entity_types.push(t.clone());
                    }
                }
            }
            continue;
        }
        if key.starts_with('@') {
            continue;
        }

        let patch_instances = match value.as_array() {
            Some(arr) => arr,
            None => {
                entity_obj.insert(key.clone(), value.clone());
                continue;
            }
        };

        if !patch_instances.iter().any(instance_is_null_sentinel) {
            deep_merge_attribute(entity_obj, key, patch_instances);
            continue;
        }

        // Process null sentinel instances
        for patch_instance in patch_instances {
            if instance_is_null_sentinel(patch_instance) {
                let dataset_id = get_instance_dataset_id(patch_instance);
                if dataset_id.is_none() {
                    // No datasetId on null sentinel → delete ALL instances
                    entity_obj.remove(key);
                    break;
                } else {
                    remove_instances_by_dataset(entity_obj, key, dataset_id.as_deref());
                }
            }
        }
    }

    Ok(())
}

/// Append attributes (§5.6.3): add attributes from `attrs` to `entity`.
/// With `no_overwrite`, existing instances (matched by datasetId) are kept;
/// new instances with a different datasetId are still appended.
/// Without `no_overwrite`, instances are merged by datasetId.
///
/// Returns `(not_appended)` — a list of attribute IRIs where at least one
/// instance was NOT appended (because `no_overwrite` was set and the instance
/// already existed).
pub fn append_entity_attrs(entity: &mut Value, attrs: &Value, no_overwrite: bool) -> Result<Vec<String>, String> {
    let entity_obj = expanded_object_mut(entity).ok_or("Entity data is not a valid JSON-LD expanded value")?;
    let attrs_obj = attrs.as_object().ok_or("Attributes must be a JSON object")?;

    let mut not_appended = Vec::new();

    for (key, value) in attrs_obj {
        if key.starts_with('@') {
            continue;
        }

        // Scope: merge arrays with deduplication
        if key == iri::SCOPE.as_str() {
            if let (Some(Value::Array(existing_scopes)), Some(new_scopes)) = (entity_obj.get_mut(key), value.as_array()) {
                if no_overwrite {
                    // noOverwrite: add scope values that don't already exist
                    for s in new_scopes {
                        if !existing_scopes.contains(s) {
                            existing_scopes.push(s.clone());
                        }
                    }
                } else {
                    // overwrite: replace scope
                    entity_obj.insert(key.clone(), value.clone());
                }
            } else {
                entity_obj.insert(key.clone(), value.clone());
            }
            continue;
        }

        if !entity_obj.contains_key(key) {
            // Attribute doesn't exist yet — just add it
            entity_obj.insert(key.clone(), value.clone());
            continue;
        }

        // Attribute already exists — merge by datasetId
        let patch_instances = match value.as_array() {
            Some(arr) => arr,
            None => {
                if !no_overwrite {
                    entity_obj.insert(key.clone(), value.clone());
                } else {
                    not_appended.push(key.clone());
                }
                continue;
            }
        };

        if no_overwrite {
            // Only add instances whose datasetId doesn't already exist
            let Some(Value::Array(existing_arr)) = entity_obj.get_mut(key) else {
                entity_obj.insert(key.clone(), value.clone());
                continue;
            };
            let mut any_skipped = false;
            for patch_inst in patch_instances {
                let patch_ds = get_instance_dataset_id(patch_inst);
                let already_exists = existing_arr.iter().any(|inst| get_instance_dataset_id(inst) == patch_ds);
                if !already_exists {
                    existing_arr.push(patch_inst.clone());
                } else {
                    any_skipped = true;
                }
            }
            if any_skipped {
                not_appended.push(key.clone());
            }
        } else {
            deep_merge_attribute(entity_obj, key, patch_instances);
        }
    }

    Ok(not_appended)
}

/// Update entity attributes (§5.6.2).
///
/// For regular attributes: if the attribute exists, update by datasetId
/// matching; if not, add it. For scope: if scope doesn't exist on the
/// entity, report as not updated.
///
/// Returns a list of attribute IRIs that were NOT updated (e.g. scope not
/// present). The caller uses this to decide between 204 and 207.
pub fn update_entity_attrs(entity: &mut Value, attrs: &Value) -> Result<Vec<String>, String> {
    let entity_obj = expanded_object_mut(entity).ok_or("Entity data is not a valid JSON-LD expanded value")?;
    let attrs_obj = attrs.as_object().ok_or("Attributes must be a JSON object")?;

    let mut not_updated = Vec::new();

    for (key, value) in attrs_obj {
        // Merge @type arrays (union, deduplicate) so entity type additions work
        if key == "@type" {
            if let (Some(entity_types), Some(patch_types)) = (entity_obj.get_mut("@type").and_then(Value::as_array_mut), value.as_array()) {
                for t in patch_types {
                    if !entity_types.contains(t) {
                        entity_types.push(t.clone());
                    }
                }
            }
            continue;
        }
        if key.starts_with('@') {
            continue;
        }

        // Scope: only update if already present, otherwise report as not updated
        if key == iri::SCOPE.as_str() {
            if entity_obj.contains_key(key) {
                entity_obj.insert(key.clone(), value.clone());
            } else {
                not_updated.push(key.clone());
            }
            continue;
        }

        // If the attribute doesn't exist yet, add it
        if !entity_obj.contains_key(key) {
            entity_obj.insert(key.clone(), value.clone());
            continue;
        }

        let patch_instances = match value.as_array() {
            Some(arr) => arr,
            None => {
                entity_obj.insert(key.clone(), value.clone());
                continue;
            }
        };

        if patch_instances.iter().any(instance_is_null_sentinel) {
            for patch_instance in patch_instances {
                if instance_is_null_sentinel(patch_instance) {
                    let dataset_id = get_instance_dataset_id(patch_instance);
                    remove_instances_by_dataset(entity_obj, key, dataset_id.as_deref());
                } else {
                    deep_merge_attribute(entity_obj, key, std::slice::from_ref(patch_instance));
                }
            }
        } else {
            // Update by datasetId: replace matching instances, preserve others
            deep_merge_attribute(entity_obj, key, patch_instances);
        }
    }

    Ok(not_updated)
}

/// Partial update of a single attribute (§5.6.4): merge `fragment` into the
/// attribute identified by `attr_iri`, optionally targeting a specific
/// `dataset_id`.
pub fn partial_update_attr(entity: &mut Value, attr_iri: &str, dataset_id: Option<&str>, fragment: &Value) -> Result<(), String> {
    let entity_obj = expanded_object_mut(entity).ok_or("Entity data is not a valid JSON-LD expanded value")?;

    if !entity_obj.contains_key(attr_iri) {
        return Err(format!("Attribute {} not found", attr_iri));
    }

    // Determine the effective datasetId: prefer the explicit query parameter,
    // fall back to the datasetId inside the fragment itself.
    let fragment_ds = get_instance_dataset_id(fragment);
    let effective_ds: Option<&str> = dataset_id.or(fragment_ds.as_deref());

    if instance_is_null_sentinel(fragment) {
        remove_instances_by_dataset(entity_obj, attr_iri, effective_ds);
        return Ok(());
    }

    // Extract the fragment's @type (if present) for type-mismatch validation
    let fragment_types: Vec<&str> = fragment
        .get("@type")
        .and_then(|t| t.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    if let Some(ds_id) = effective_ds {
        let Some(Value::Array(instances)) = entity_obj.get_mut(attr_iri) else {
            return Err(format!("Attribute {} has no instances", attr_iri));
        };

        let found = instances.iter_mut().find(|instance| {
            instance
                .get(iri::DATASET_ID.as_str())
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|obj| obj.get("@id"))
                .and_then(|id| id.as_str())
                == Some(ds_id)
        });

        match found {
            Some(instance) => {
                validate_type_match(&fragment_types, instance, attr_iri)?;
                merge_json(instance, fragment);
            }
            None => {
                return Err(format!("Attribute {} with datasetId {} not found", attr_iri, ds_id));
            }
        }
    } else {
        // No datasetId: target the default instance (the one without datasetId)
        let Some(Value::Array(instances)) = entity_obj.get_mut(attr_iri) else {
            let attr_value = entity_obj.get_mut(attr_iri).unwrap();
            if !fragment_types.is_empty() {
                validate_type_match(&fragment_types, attr_value, attr_iri)?;
            }
            merge_json(attr_value, fragment);
            return Ok(());
        };

        let found = instances.iter_mut().find(|instance| get_instance_dataset_id(instance).is_none());

        match found {
            Some(instance) => {
                validate_type_match(&fragment_types, instance, attr_iri)?;
                merge_json(instance, fragment);
            }
            None => {
                return Err(format!("Attribute {} default instance not found", attr_iri));
            }
        }
    }

    Ok(())
}

/// Check that the fragment's @type (if specified) matches the instance's @type.
fn validate_type_match(fragment_types: &[&str], instance: &Value, attr_iri: &str) -> Result<(), String> {
    if fragment_types.is_empty() {
        return Ok(());
    }
    let instance_types: Vec<&str> = instance
        .get("@type")
        .and_then(|t| t.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    if instance_types.is_empty() {
        return Ok(());
    }
    // Check if fragment type intersects with instance type
    if fragment_types.iter().any(|ft| instance_types.contains(ft)) {
        Ok(())
    } else {
        Err(format!("Attribute type mismatch for {}", attr_iri))
    }
}

/// Delete a single attribute (§5.6.5).
///
/// - `delete_all`: remove the key entirely.
/// - `dataset_id`: remove only the matching instance.
/// - Neither: remove the default (no-datasetId) instance.
pub fn delete_attr(entity: &mut Value, attr_iri: &str, dataset_id: Option<&str>, delete_all: bool) -> Result<(), String> {
    let entity_obj = expanded_object_mut(entity).ok_or("Entity data is not a valid JSON-LD expanded value")?;

    if !entity_obj.contains_key(attr_iri) {
        return Err(format!("Attribute {} not found", attr_iri));
    }

    if delete_all {
        entity_obj.remove(attr_iri);
    } else if !remove_instances_by_dataset(entity_obj, attr_iri, dataset_id) {
        return match dataset_id {
            Some(ds_id) => Err(format!("Attribute {} with datasetId {} not found", attr_iri, ds_id)),
            None => Err(format!("Attribute {} default instance not found", attr_iri)),
        };
    }

    Ok(())
}

/// Replace a single attribute (§5.6.19): fully replaces the attribute value.
pub fn replace_attr(entity: &mut Value, attr_iri: &str, value: &Value) -> Result<(), String> {
    let entity_obj = expanded_object_mut(entity).ok_or("Entity data is not a valid JSON-LD expanded value")?;

    let existing = entity_obj.get(attr_iri).ok_or_else(|| format!("Attribute {} not found", attr_iri))?;

    let new_ds = get_instance_dataset_id(value);

    let existing_instances = existing.as_array();

    if let Some(instances) = existing_instances {
        let mut new_instances: Vec<Value> = Vec::with_capacity(instances.len());
        let mut replaced = false;

        for inst in instances {
            let inst_ds = get_instance_dataset_id(inst);
            if inst_ds == new_ds {
                new_instances.push(value.clone());
                replaced = true;
            } else {
                new_instances.push(inst.clone());
            }
        }

        if !replaced {
            return Err(format!("Attribute {} not found", attr_iri));
        }

        entity_obj.insert(attr_iri.to_string(), Value::Array(new_instances));
    } else {
        entity_obj.insert(attr_iri.to_string(), Value::Array(vec![value.clone()]));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn null_sentinel_has_value() {
        let instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "urn:ngsi-ld:null"}]
        });
        assert!(instance_is_null_sentinel(&instance));
    }

    #[test]
    fn null_sentinel_has_object() {
        let instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
            "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:null"}]
        });
        assert!(instance_is_null_sentinel(&instance));
    }

    #[test]
    fn non_null_is_not_sentinel() {
        let instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 42}]
        });
        assert!(!instance_is_null_sentinel(&instance));
    }

    #[test]
    fn extract_dataset_id() {
        let instance = json!({
            "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:A"}]
        });
        assert_eq!(get_instance_dataset_id(&instance).as_deref(), Some("urn:ngsi-ld:Dataset:A"));
    }

    #[test]
    fn extract_scopes_from_entity() {
        let entity = json!([{
            "@type": ["https://example.com/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/scope": [
                {"@value": "/Madrid"},
                {"@value": "/Traffic"}
            ]
        }]);
        let scopes = extract_scopes(&entity);
        assert_eq!(scopes, vec!["/Madrid", "/Traffic"]);
    }

    #[test]
    fn merge_entity_adds_new_attr() {
        let mut entity = json!([{"@type": ["https://example.com/T"], "https://example.com/a": [{"@value": 1}]}]);
        let patch = json!([{"https://example.com/b": [{"@value": 2}]}]);
        merge_entity(&mut entity, &patch).unwrap();
        assert!(entity[0].get("https://example.com/b").is_some());
    }

    #[test]
    fn append_no_overwrite_skips_existing() {
        let mut entity = json!([{"https://example.com/a": [{"@value": 1}]}]);
        let attrs = json!({"https://example.com/a": [{"@value": 99}], "https://example.com/b": [{"@value": 2}]});
        append_entity_attrs(&mut entity, &attrs, true).unwrap();
        assert_eq!(entity[0]["https://example.com/a"][0]["@value"], 1);
        assert_eq!(entity[0]["https://example.com/b"][0]["@value"], 2);
    }

    #[test]
    fn delete_attr_removes_key() {
        let mut entity = json!([{"https://example.com/a": [{"@value": 1}]}]);
        delete_attr(&mut entity, "https://example.com/a", None, true).unwrap();
        assert!(entity[0].get("https://example.com/a").is_none());
    }

    #[test]
    fn delete_attr_not_found() {
        let mut entity = json!([{"https://example.com/a": [{"@value": 1}]}]);
        let result = delete_attr(&mut entity, "https://example.com/missing", None, true);
        assert!(result.is_err());
    }
}
