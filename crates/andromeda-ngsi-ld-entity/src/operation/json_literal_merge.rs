//! Merging the raw JSON a JSON literal carries (clauses 4.5.24 and 5.5.12).
//!
//! A `JsonProperty`'s `json` member is "a raw JSON object (or array of objects)
//! which contains data which is not available for JSON-LD interpretation"
//! (clause 4.5.24.2), so expansion keeps it as a single `@json` literal instead
//! of walking into it. Clause 5.5.12 still merges "up to an arbitrary depth,
//! e.g. including going into JSON objects representing a Property value", and
//! clause 5.5.4 excepts merge operations alone from its prohibition of the
//! NGSI-LD Null "as the value of a key value pair within a JSON object", which
//! would be pointless if a merge could not reach inside such an object.
//!
//! The deletion token inside a JSON literal is the NGSI-LD Null *string*, never
//! a JSON `null`. Clause 4.5.0 introduces the URI precisely because "null
//! cannot be used as a value in NGSI-LD", and inside a `@json` literal a JSON
//! `null` survives expansion untouched, so it is ordinary data.

use andromeda_ngsi_ld_vocabulary::NULL_URI;
use serde_json::{Map, Value};

/// Apply one raw JSON patch to one raw JSON target, per IETF RFC 7396 with the
/// NGSI-LD Null of clause 4.5.0 in place of `null`.
///
/// A patch that is not an object replaces the target outright, as RFC 7396
/// states; a target that is not an object is replaced by the object the patch
/// describes, because there is nothing in it to merge into.
pub fn merge_json_literal(target: &mut Value, patch: &Value) {
    let Some(patch_members) = patch.as_object() else {
        // clone: the patch is borrowed and the target takes ownership.
        *target = patch.clone();
        return;
    };
    if !target.is_object() {
        *target = Value::Object(Map::new());
    }
    let Some(target_members) = target.as_object_mut() else {
        return;
    };
    for (key, patched) in patch_members {
        if patched.as_str() == Some(NULL_URI.as_str()) {
            target_members.remove(key);
            continue;
        }
        // RFC 7396 section 2 recurses into every object the patch carries,
        // whether or not the target holds the key, so a Null nested under a
        // key the target lacks is dropped rather than written as data.
        match (target_members.get_mut(key), patched.is_object()) {
            (Some(existing), true) => merge_json_literal(existing, patched),
            (None, true) => {
                let mut fresh = Value::Null;
                merge_json_literal(&mut fresh, patched);
                // clone: the key is borrowed from the patch.
                target_members.insert(key.clone(), fresh);
            }
            // clone: both the key and its value are borrowed from the patch.
            (Some(_) | None, false) => {
                target_members.insert(key.clone(), patched.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::operation::json_literal_merge::merge_json_literal;
    use serde_json::json;

    /// The fixture of ETSI test `056_10_01`: a JSON object holding a genuine JSON
    /// `null` alongside ordinary members.
    fn stored_json() -> serde_json::Value {
        json!({"id": "some-id", "type": "some-type", "null": null, "anythingElse": "anything-else"})
    }

    /// Clause 5.5.12 removes the key the patch set to the NGSI-LD Null and
    /// leaves every other key of the stored object alone, including the one
    /// whose value is a genuine JSON `null`, which is data rather than a
    /// deletion (clause 4.5.0).
    #[test]
    fn a_key_set_to_the_null_sentinel_is_removed_and_the_rest_survive() {
        let mut target = stored_json();
        merge_json_literal(&mut target, &json!({"anythingElse": "urn:ngsi-ld:null"}));
        pretty_assertions::assert_eq!(target, json!({"id": "some-id", "type": "some-type", "null": null}));
    }

    /// IETF RFC 7396: a key the patch names is replaced and a key it omits
    /// survives.
    #[test]
    fn a_named_key_is_replaced_and_an_omitted_one_survives() {
        let mut target = stored_json();
        merge_json_literal(&mut target, &json!({"id": "another-id"}));
        pretty_assertions::assert_eq!(target["id"], json!("another-id"));
        pretty_assertions::assert_eq!(target["anythingElse"], json!("anything-else"));
    }

    /// Clause 5.5.12 merges "up to an arbitrary depth", so a nested object is
    /// merged rather than replaced.
    #[test]
    fn a_nested_object_is_merged_rather_than_replaced() {
        let mut target = json!({"outer": {"kept": 1, "replaced": 2, "removed": 3}});
        merge_json_literal(&mut target, &json!({"outer": {"replaced": 9, "removed": "urn:ngsi-ld:null"}}));
        pretty_assertions::assert_eq!(target, json!({"outer": {"kept": 1, "replaced": 9}}));
    }

    /// Clause 4.5.0: a JSON `null` is not the NGSI-LD Null, so a patch carrying
    /// one writes it as the key's value.
    #[test]
    fn a_json_null_in_the_patch_is_written_as_data() {
        let mut target = json!({"present": 1});
        merge_json_literal(&mut target, &json!({"present": null}));
        pretty_assertions::assert_eq!(target, json!({"present": null}));
    }

    /// IETF RFC 7396 section 2 recurses into an object the target lacks too, so
    /// a Null nested under a new key is dropped rather than stored as data
    /// (clause 4.5.0).
    #[test]
    fn a_null_nested_under_a_key_the_target_lacks_is_dropped() {
        let mut target = json!({"kept": 1});
        merge_json_literal(&mut target, &json!({"added": {"a": 2, "b": "urn:ngsi-ld:null"}}));
        pretty_assertions::assert_eq!(target, json!({"kept": 1, "added": {"a": 2}}));
    }

    /// IETF RFC 7396: a patch that is not an object replaces the target, and a
    /// target that is not an object is replaced by the patch's own object.
    #[test]
    fn a_non_object_on_either_side_replaces_rather_than_merges() {
        let mut array_target = json!([1, 2, 3]);
        merge_json_literal(&mut array_target, &json!({"a": 1}));
        pretty_assertions::assert_eq!(array_target, json!({"a": 1}));

        let mut object_target = json!({"a": 1});
        merge_json_literal(&mut object_target, &json!([4, 5]));
        pretty_assertions::assert_eq!(object_target, json!([4, 5]));
    }
}
