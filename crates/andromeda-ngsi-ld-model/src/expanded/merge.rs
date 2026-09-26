//! Merging expanded JSON-LD fragments into a stored payload.

use crate::expanded::{
    dataset_id::{InstanceDatasetIdError, get_instance_dataset_id, instance_dataset_ids, position_of_dataset},
    json_literal_merge::merge_json_literal,
    null_sentinel::{instance_is_null_sentinel, member_is_null_sentinel},
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use serde_json::{Map, Value};

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
///
/// # Errors
///
/// Returns [`InstanceDatasetIdError`] when a patch instance, or a stored
/// instance consulted before its match, carries a `datasetId` that is not an
/// identifier: matching it to no instance would append a second one, and
/// reading it as absent would patch the default instance (clause 4.5.5).
pub fn deep_merge_attribute(
    entity_obj: &mut Map<String, Value>,
    key: &str,
    patch_instances: &[Value],
    depth: InstancePatchDepth,
) -> Result<(), InstanceDatasetIdError> {
    let existing = entity_obj.get_mut(key);
    let Some(existing) = existing else {
        entity_obj.insert(key.to_string(), Value::Array(patch_instances.to_vec()));
        return Ok(());
    };
    let Some(existing_arr) = existing.as_array_mut() else {
        entity_obj.insert(key.to_string(), Value::Array(patch_instances.to_vec()));
        return Ok(());
    };

    for patch_inst in patch_instances {
        let patch_ds_id = get_instance_dataset_id(patch_inst)?;
        let matched = position_of_dataset(existing_arr, patch_ds_id.as_ref())?.and_then(|position| existing_arr.get_mut(position));

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
    Ok(())
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
/// key — every member but `datasetId`, which clause 5.5.12 states cannot be
/// deleted that way — and every other member recurses so a Property value
/// object is merged to arbitrary depth.
fn merge_node(base: &mut Value, patch: &Value) {
    // A `JsonProperty`'s value is a leaf to JSON-LD and a structure to clause
    // 5.5.12, which merges "up to an arbitrary depth, e.g. including going into
    // JSON objects representing a Property value". Expansion never walks into a
    // `@json` literal, so the merge has to be performed on the raw value.
    if let (Some(base_json), Some(patch_json)) = (json_literal_value_mut(base), json_literal_value(patch)) {
        merge_json_literal(base_json, patch_json);
        return;
    }
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
            // Clause 5.5.12 closes its deletion rule with one member it does
            // not reach: "A datasetId cannot be deleted by setting it to the
            // value urn:ngsi-ld:null." Removing it here would move the instance
            // to the default one of clause 4.5.5, silently rewriting an
            // identity the fragment was never allowed to touch.
            if key != properties::DATASET_ID.as_str() {
                base_obj.remove(key);
            }
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

/// The raw JSON an expanded `@json` literal node carries, if the node is one.
///
/// Clause 4.5.24.2 keeps a `JsonProperty`'s value "not available for JSON-LD
/// interpretation", which expansion expresses as `{"@type": "@json", "@value":
/// <raw>}`.
fn json_literal_value(node: &Value) -> Option<&Value> {
    let members = node.as_object()?;
    (members.get("@type").and_then(Value::as_str) == Some("@json")).then(|| members.get("@value"))?
}

/// The mutable twin of [`json_literal_value`].
fn json_literal_value_mut(node: &mut Value) -> Option<&mut Value> {
    let members = node.as_object_mut()?;
    if members.get("@type").and_then(Value::as_str) != Some("@json") {
        return None;
    }
    members.get_mut("@value")
}

/// Whether an array-wrapped member of a merge fragment deletes the key it
/// stands on.
///
/// Clause 5.5.12 removes "each member of the Fragment, whose value is an
/// NGSI-LD Null", and clause 4.5.0 gives that value two spellings. A
/// non-reified member -- an `observedAt`, a `unitCode` -- carries the bare URI
/// and expands to a plain node. A sub-attribute carries its own family's
/// deletion sentinel instead: clauses 4.5.2, 4.5.3, 4.5.18, 4.5.21 and 4.5.22
/// each state that an NGSI-LD Null on the right-hand side of the family's value
/// member indicates "a deletion of the Property" -- of the whole sub-attribute,
/// not of its value alone -- so recognising only the first spelling would leave
/// behind a valueless instance the fragment asked to remove.
///
/// Clause 4.5.0 admits exactly one NGSI-LD Null where an array is required, so
/// a member holding more than one node is data even when one of them is the
/// sentinel.
fn member_is_null(member: &Value) -> bool {
    let Some(nodes) = member.as_array() else {
        return member_is_null_sentinel(member) || instance_is_null_sentinel(member);
    };
    nodes.len() == 1 && nodes.iter().all(|node| member_is_null_sentinel(node) || instance_is_null_sentinel(node))
}

/// Remove instances matching a dataset ID (or the default instance if `None`)
/// from the instances array for an attribute key. Removes the key entirely if
/// no instances remain.
///
/// Returns whether at least one instance was actually removed. The default
/// instance is the one carrying no `datasetId` at all.
///
/// # Errors
///
/// Returns [`InstanceDatasetIdError`], and removes nothing, when any instance
/// of the attribute carries a `datasetId` that is not an identifier: which
/// instance it names cannot be told.
pub(crate) fn remove_instances_by_dataset(entity_obj: &mut Map<String, Value>, key: &str, dataset_id: Option<&Uri>) -> Result<bool, InstanceDatasetIdError> {
    let Some(Value::Array(instances)) = entity_obj.get_mut(key) else {
        return Ok(false);
    };

    let before = instances.len();
    // `retain` visits every element exactly once in order, so the identifiers
    // read up front line up with the instances they were read from.
    let mut datasets = instance_dataset_ids(instances)?.into_iter();
    instances.retain(|_| datasets.next().is_some_and(|candidate| candidate.as_ref() != dataset_id));
    let removed = instances.len() < before;

    if instances.is_empty() {
        entity_obj.remove(key);
    }

    Ok(removed)
}

#[cfg(test)]
mod tests {
    use crate::expanded::merge::{InstancePatchDepth, deep_merge_attribute, remove_instances_by_dataset};
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
    fn deep_merging_an_absent_attribute_inserts_the_patch_instances() {
        let mut object = Map::new();
        deep_merge_attribute(&mut object, "https://example.com/speed", &[instance(10, None)], InstancePatchDepth::DeepMerge).expect("readable datasetIds");
        pretty_assertions::assert_eq!(object["https://example.com/speed"], json!([instance(10, None)]));
    }

    #[test]
    fn deep_merging_over_a_non_array_member_replaces_it() {
        let mut object = Map::new();
        object.insert("https://example.com/speed".to_string(), json!("not an instance array"));
        deep_merge_attribute(&mut object, "https://example.com/speed", &[instance(10, None)], InstancePatchDepth::DeepMerge).expect("readable datasetIds");
        pretty_assertions::assert_eq!(object["https://example.com/speed"], json!([instance(10, None)]));
    }

    #[test]
    fn deep_merging_a_matching_dataset_id_merges_the_instance_members() {
        let mut object = entity_with(vec![instance(10, Some("urn:ngsi-ld:Dataset:A"))]);
        let mut patch = instance(20, Some("urn:ngsi-ld:Dataset:A"));
        patch["https://uri.etsi.org/ngsi-ld/unitCode"] = json!([{"@value": "KMH"}]);
        deep_merge_attribute(&mut object, "https://example.com/speed", &[patch], InstancePatchDepth::DeepMerge).expect("readable datasetIds");

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
        deep_merge_attribute(&mut object, "https://example.com/speed", &[patch.clone()], InstancePatchDepth::DeepMerge).expect("readable datasetIds"); // clone: the patch is compared below

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
        )
        .expect("readable datasetIds");
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
        deep_merge_attribute(&mut object, "https://example.com/speed", &[patch], InstancePatchDepth::DeepMerge).expect("readable datasetIds");

        let value = &object["https://example.com/speed"][0]["https://uri.etsi.org/ngsi-ld/hasValue"][0];
        pretty_assertions::assert_eq!(value["https://example.com/street"], json!([{"@value": "new"}]));
        pretty_assertions::assert_eq!(value["https://example.com/city"], json!([{"@value": "town"}]));
        assert!(value.get("https://example.com/country").is_none(), "the null-sentinel member is deleted");
    }

    #[test]
    fn deep_merge_never_deletes_a_dataset_id_with_the_null_sentinel() {
        // ETSI GS CIM 009 v1.9.1 clause 5.5.12: "A datasetId cannot be deleted
        // by setting it to the value urn:ngsi-ld:null." The deletion rule this
        // clause states in the bullet above therefore steps over that one
        // member: deleting it would move the instance onto the default
        // instance of clause 4.5.5, which is a different instance altogether.
        // The write paths refuse such a fragment before it reaches here
        // (clause 5.5.4), so the fixture is an instance already carrying the
        // sentinel, which is what makes the two identities match.
        let mut object = entity_with(vec![instance(10, Some("urn:ngsi-ld:null"))]);
        let patch = json!({
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 20}],
            "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:null"}]
        });
        deep_merge_attribute(&mut object, "https://example.com/speed", &[patch], InstancePatchDepth::DeepMerge).expect("readable datasetIds");

        let instances = object["https://example.com/speed"].as_array().expect("instances");
        pretty_assertions::assert_eq!(instances.len(), 1);
        pretty_assertions::assert_eq!(
            instances[0]["https://uri.etsi.org/ngsi-ld/datasetId"],
            json!([{"@id": "urn:ngsi-ld:null"}]),
            "the datasetId member survives the merge"
        );
        pretty_assertions::assert_eq!(instances[0]["https://uri.etsi.org/ngsi-ld/hasValue"], json!([{"@value": 20}]));
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
        deep_merge_attribute(&mut object, "https://example.com/speed", &[patch], InstancePatchDepth::DeepMerge).expect("readable datasetIds");

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
        deep_merge_attribute(&mut object, "https://example.com/speed", &[patch], InstancePatchDepth::ReplaceInstance).expect("readable datasetIds");

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
        assert!(remove_instances_by_dataset(&mut object, "https://example.com/speed", dataset_id.as_ref()).expect("readable datasetIds"));
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
        assert!(remove_instances_by_dataset(&mut object, "https://example.com/speed", None).expect("readable datasetIds"));
        let instances = object["https://example.com/speed"].as_array().expect("instances");
        pretty_assertions::assert_eq!(instances.len(), 1);
        pretty_assertions::assert_eq!(instances[0]["https://uri.etsi.org/ngsi-ld/hasValue"], json!([{"@value": 10}]));
    }

    #[test]
    fn removing_an_unmatched_dataset_id_reports_that_nothing_was_removed() {
        let mut object = entity_with(vec![instance(10, Some("urn:ngsi-ld:Dataset:A"))]);
        let dataset_id = "urn:ngsi-ld:Dataset:Z".parse::<Uri>().ok();
        assert!(!remove_instances_by_dataset(&mut object, "https://example.com/speed", dataset_id.as_ref()).expect("readable datasetIds"));
        pretty_assertions::assert_eq!(object["https://example.com/speed"].as_array().expect("instances").len(), 1);
    }

    #[test]
    fn removing_the_last_instance_drops_the_attribute_member_itself() {
        let mut object = entity_with(vec![instance(10, Some("urn:ngsi-ld:Dataset:A"))]);
        let dataset_id = "urn:ngsi-ld:Dataset:A".parse::<Uri>().ok();
        assert!(remove_instances_by_dataset(&mut object, "https://example.com/speed", dataset_id.as_ref()).expect("readable datasetIds"));
        assert!(object.get("https://example.com/speed").is_none());
    }

    #[test]
    fn removing_from_an_absent_attribute_reports_that_nothing_was_removed() {
        let mut object = Map::new();
        assert!(!remove_instances_by_dataset(&mut object, "https://example.com/speed", None).expect("readable datasetIds"));
    }
}
