//! Merging expanded JSON-LD fragments into a stored payload.

use crate::expanded::{dataset_id::get_instance_dataset_id, null_sentinel::member_is_null_sentinel};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
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

/// How deeply a matched attribute instance is patched (clause 5.6).
///
/// The two whole-entity write operations that patch attribute instances by
/// `datasetId` need different depth semantics on a match, and the spec is
/// explicit about both:
///
/// - [`ReplaceInstance`](InstancePatchDepth::ReplaceInstance): Update Attributes
///   (clause 5.6.2, EXAMPLE 2 of clause 5.5.8) replaces the whole matched
///   instance, so a sub-attribute the patch omits — a `unitCode`, an
///   `observedAt` — is dropped.
/// - [`DeepMerge`](InstancePatchDepth::DeepMerge): Merge Entity (clause 5.6.17,
///   behaviour in clause 5.5.12) merges to arbitrary depth, descending into a
///   Property value object and a language map, so a member the patch omits
///   survives and a nested `urn:ngsi-ld:null` deletes just its key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstancePatchDepth {
    /// Replace the whole matched instance with the patch instance.
    ReplaceInstance,
    /// Recursively merge the patch instance into the matched instance.
    DeepMerge,
}

/// Patch attribute instances into an entity object, matching by `datasetId`.
///
/// An instance whose `datasetId` matches an existing one is patched under the
/// requested [`InstancePatchDepth`]; an instance with no match is appended. A
/// changed attribute `@type` always replaces the whole instance regardless of
/// depth, because the members of one attribute family have no meaning under
/// another (clause 5.2).
pub fn deep_merge_attribute(entity_obj: &mut Map<String, Value>, key: &str, patch_instances: &[Value], depth: InstancePatchDepth) {
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
            if instance_type_changed(existing_inst, patch_inst) {
                // clone: the patch is borrowed and the stored instance takes ownership.
                *existing_inst = patch_inst.clone();
                continue;
            }
            match depth {
                // clone: the patch is borrowed and the stored instance takes ownership.
                InstancePatchDepth::ReplaceInstance => *existing_inst = patch_inst.clone(),
                InstancePatchDepth::DeepMerge => merge_node(existing_inst, patch_inst),
            }
        } else {
            // clone: the patch is borrowed and the instance array takes ownership.
            existing_arr.push(patch_inst.clone());
        }
    }
}

/// Whether the patch instance carries a different attribute `@type` than the
/// existing one.
fn instance_type_changed(existing_inst: &Value, patch_inst: &Value) -> bool {
    match (existing_inst.get("@type"), patch_inst.get("@type")) {
        (Some(existing_type), Some(patch_type)) => existing_type != patch_type,
        (Some(_) | None, None) | (None, Some(_)) => false,
    }
}

/// Recursively merge the expanded node `patch` into `base` (clause 5.5.12).
///
/// A leaf node (one carrying `@value` or `@id`) is replaced wholesale. Two
/// structured nodes are merged member-by-member: a language map merges per
/// language tag, a member carrying the `urn:ngsi-ld:null` sentinel deletes its
/// key, and every other member recurses so a Property value object is merged to
/// arbitrary depth.
fn merge_node(base: &mut Value, patch: &Value) {
    // Only two non-leaf structured nodes deep-merge; a leaf value or any shape
    // mismatch is atomic and the patch replaces the base wholesale.
    let mergeable = matches!((base.as_object(), patch.as_object()), (Some(base_obj), Some(patch_obj)) if !is_leaf_node(base_obj) && !is_leaf_node(patch_obj));
    if !mergeable {
        // clone: the patch replaces the base.
        *base = patch.clone();
        return;
    }

    let (Some(base_obj), Some(patch_obj)) = (base.as_object_mut(), patch.as_object()) else {
        return;
    };
    for (key, patch_member) in patch_obj {
        if key == properties::LANGUAGE_MAP.as_str() {
            merge_language_map(base_obj, key, patch_member);
            continue;
        }
        if key.starts_with('@') {
            // clone: JSON-LD keywords (`@type`) are copied across verbatim.
            base_obj.insert(key.clone(), patch_member.clone());
            continue;
        }
        if member_is_null(patch_member) {
            base_obj.remove(key);
            continue;
        }
        match base_obj.get_mut(key) {
            Some(existing_member) => merge_member(existing_member, patch_member),
            // clone: a member absent from the base is taken from the patch.
            None => {
                base_obj.insert(key.clone(), patch_member.clone());
            }
        }
    }
}

/// Merge two array-wrapped expanded members. A single-node array on both sides
/// recurses into the node; any other shape (a multi-instance member, a shape
/// mismatch) is replaced wholesale.
fn merge_member(base: &mut Value, patch: &Value) {
    match (base.as_array_mut(), patch.as_array()) {
        (Some(base_arr), Some(patch_arr)) if base_arr.len() == 1 && patch_arr.len() == 1 => {
            merge_node(&mut base_arr[0], &patch_arr[0]);
        }
        // clone: nothing to recurse into, so the patch member replaces the base.
        (Some(_) | None, Some(_) | None) => *base = patch.clone(),
    }
}

/// Merge a language map member by language tag (clause 4.5.6). An entry whose
/// value is the `urn:ngsi-ld:null` sentinel deletes that tag; every other entry
/// replaces the entry with the same tag or is added.
fn merge_language_map(base_obj: &mut Map<String, Value>, key: &str, patch_member: &Value) {
    let Some(patch_entries) = patch_member.as_array() else {
        // clone: a non-array language map cannot be merged per tag, so it replaces.
        base_obj.insert(key.to_owned(), patch_member.clone());
        return;
    };
    let Some(Value::Array(base_entries)) = base_obj.get_mut(key) else {
        // clone: the base carries no language map to merge into, so the patch is taken.
        base_obj.insert(key.to_owned(), patch_member.clone());
        return;
    };
    for patch_entry in patch_entries {
        let language = patch_entry.get("@language");
        if member_is_null_sentinel(patch_entry) {
            base_entries.retain(|entry| entry.get("@language") != language);
            continue;
        }
        match base_entries.iter_mut().find(|entry| entry.get("@language") == language) {
            // clone: the patch entry replaces the base entry carrying the same tag.
            Some(existing) => *existing = patch_entry.clone(),
            // clone: a tag absent from the base is taken from the patch.
            None => base_entries.push(patch_entry.clone()),
        }
    }
}

/// Whether an expanded node is a leaf carrying an atomic value.
fn is_leaf_node(node: &Map<String, Value>) -> bool {
    node.contains_key("@value") || node.contains_key("@id")
}

/// Whether an array-wrapped member is the `urn:ngsi-ld:null` deletion sentinel.
fn member_is_null(member: &Value) -> bool {
    member.as_array().and_then(|entries| entries.first()).is_some_and(member_is_null_sentinel)
}

/// Remove instances matching a dataset ID (or the default instance if `None`)
/// from the instances array for an attribute key. Removes the key entirely if
/// no instances remain.
///
/// Returns whether at least one instance was actually removed.
pub(crate) fn remove_instances_by_dataset(entity_obj: &mut Map<String, Value>, key: &str, dataset_id: Option<&Uri>) -> bool {
    let Some(Value::Array(instances)) = entity_obj.get_mut(key) else {
        return false;
    };

    let before = instances.len();
    match dataset_id {
        Some(ds_id) => {
            instances.retain(|inst| get_instance_dataset_id(inst).as_ref() != Some(ds_id));
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
    use crate::expanded::merge::{InstancePatchDepth, deep_merge_attribute, merge_json, remove_instances_by_dataset};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
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
        deep_merge_attribute(&mut object, "https://example.com/speed", &[instance(10, None)], InstancePatchDepth::DeepMerge);
        pretty_assertions::assert_eq!(object["https://example.com/speed"], json!([instance(10, None)]));
    }

    #[test]
    fn deep_merging_over_a_non_array_member_replaces_it() {
        let mut object = Map::new();
        object.insert("https://example.com/speed".to_string(), json!("not an instance array"));
        deep_merge_attribute(&mut object, "https://example.com/speed", &[instance(10, None)], InstancePatchDepth::DeepMerge);
        pretty_assertions::assert_eq!(object["https://example.com/speed"], json!([instance(10, None)]));
    }

    #[test]
    fn deep_merging_a_matching_dataset_id_merges_the_instance_members() {
        let mut object = entity_with(vec![instance(10, Some("urn:ngsi-ld:Dataset:A"))]);
        let mut patch = instance(20, Some("urn:ngsi-ld:Dataset:A"));
        patch["https://uri.etsi.org/ngsi-ld/unitCode"] = json!([{"@value": "KMH"}]);
        deep_merge_attribute(&mut object, "https://example.com/speed", &[patch], InstancePatchDepth::DeepMerge);

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
        deep_merge_attribute(&mut object, "https://example.com/speed", &[patch.clone()], InstancePatchDepth::DeepMerge); // clone: the patch is compared below

        pretty_assertions::assert_eq!(object["https://example.com/speed"], json!([patch]));
    }

    #[test]
    fn deep_merging_an_unmatched_dataset_id_appends_a_new_instance() {
        let mut object = entity_with(vec![instance(10, Some("urn:ngsi-ld:Dataset:A"))]);
        deep_merge_attribute(
            &mut object,
            "https://example.com/speed",
            &[instance(20, Some("urn:ngsi-ld:Dataset:B"))],
            InstancePatchDepth::DeepMerge,
        );
        pretty_assertions::assert_eq!(object["https://example.com/speed"].as_array().expect("instances").len(), 2);
    }

    #[test]
    fn deep_merge_recurses_into_a_property_value_object() {
        // ETSI GS CIM 009 v1.9.1 clause 5.5.12 EXAMPLE 2: a Merge Entity that
        // touches one member of a Property value object keeps the untouched
        // members and deletes a member set to the `urn:ngsi-ld:null` sentinel.
        let mut object = entity_with(vec![json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{
                "https://example.com/street": [{"@value": "old"}],
                "https://example.com/city": [{"@value": "town"}],
                "https://example.com/country": [{"@value": "land"}]
            }]
        })]);
        let patch = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{
                "https://example.com/street": [{"@value": "new"}],
                "https://example.com/country": [{"@value": "urn:ngsi-ld:null"}]
            }]
        });
        deep_merge_attribute(&mut object, "https://example.com/speed", &[patch], InstancePatchDepth::DeepMerge);

        let value = &object["https://example.com/speed"][0]["https://uri.etsi.org/ngsi-ld/hasValue"][0];
        pretty_assertions::assert_eq!(value["https://example.com/street"], json!([{"@value": "new"}]));
        pretty_assertions::assert_eq!(value["https://example.com/city"], json!([{"@value": "town"}]));
        assert!(value.get("https://example.com/country").is_none(), "the null-sentinel member is deleted");
    }

    #[test]
    fn deep_merge_of_a_language_tag_keeps_the_other_tags() {
        // A LanguageProperty merge naming one tag replaces that tag and leaves
        // the rest of the language map standing (clause 4.5.6).
        let mut object = entity_with(vec![json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/LanguageProperty"],
            "https://uri.etsi.org/ngsi-ld/hasLanguageMap": [
                {"@language": "en", "@value": "hi"},
                {"@language": "de", "@value": "hallo"}
            ]
        })]);
        let patch = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/LanguageProperty"],
            "https://uri.etsi.org/ngsi-ld/hasLanguageMap": [{"@language": "de", "@value": "guten tag"}]
        });
        deep_merge_attribute(&mut object, "https://example.com/speed", &[patch], InstancePatchDepth::DeepMerge);

        let map = object["https://example.com/speed"][0]["https://uri.etsi.org/ngsi-ld/hasLanguageMap"]
            .as_array()
            .expect("language map")
            .clone();
        assert!(map.contains(&json!({"@language": "en", "@value": "hi"})), "the untouched tag is kept: {map:?}");
        assert!(
            map.contains(&json!({"@language": "de", "@value": "guten tag"})),
            "the named tag is replaced: {map:?}"
        );
        pretty_assertions::assert_eq!(map.len(), 2);
    }

    #[test]
    fn replace_instance_drops_a_subattribute_the_patch_omits() {
        // Update Attributes (clause 5.6.2) replaces the whole matched instance,
        // so a `unitCode` the patch does not carry is dropped.
        let mut object = entity_with(vec![json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 25}],
            "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "CEL"}]
        })]);
        let patch = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 100}]
        });
        deep_merge_attribute(&mut object, "https://example.com/speed", &[patch], InstancePatchDepth::ReplaceInstance);

        let instance = &object["https://example.com/speed"][0];
        pretty_assertions::assert_eq!(instance["https://uri.etsi.org/ngsi-ld/hasValue"], json!([{"@value": 100}]));
        assert!(
            instance.get("https://uri.etsi.org/ngsi-ld/unitCode").is_none(),
            "the omitted sub-attribute is dropped"
        );
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
        let dataset_id = "urn:ngsi-ld:Dataset:A".parse::<Uri>().ok();
        assert!(remove_instances_by_dataset(&mut object, "https://example.com/speed", dataset_id.as_ref()));
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
        let dataset_id = "urn:ngsi-ld:Dataset:Z".parse::<Uri>().ok();
        assert!(!remove_instances_by_dataset(&mut object, "https://example.com/speed", dataset_id.as_ref()));
        pretty_assertions::assert_eq!(object["https://example.com/speed"].as_array().expect("instances").len(), 1);
    }

    #[test]
    fn removing_the_last_instance_drops_the_attribute_member_itself() {
        let mut object = entity_with(vec![instance(10, Some("urn:ngsi-ld:Dataset:A"))]);
        let dataset_id = "urn:ngsi-ld:Dataset:A".parse::<Uri>().ok();
        assert!(remove_instances_by_dataset(&mut object, "https://example.com/speed", dataset_id.as_ref()));
        assert!(object.get("https://example.com/speed").is_none());
    }

    #[test]
    fn removing_from_an_absent_attribute_reports_that_nothing_was_removed() {
        let mut object = Map::new();
        assert!(!remove_instances_by_dataset(&mut object, "https://example.com/speed", None));
    }
}
