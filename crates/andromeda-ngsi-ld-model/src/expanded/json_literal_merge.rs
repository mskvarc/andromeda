//! Merging the raw JSON a `JsonProperty` carries (clauses 4.5.24 and 5.5.12).
//!
//! A `JsonProperty`'s `json` member is "a raw JSON object (or array of objects)
//! which contains data which is not available for JSON-LD interpretation"
//! (clause 4.5.24.2), so expansion wraps it as a single `@json` literal instead
//! of walking into it. Every other member of an attribute instance is therefore
//! merged by [`crate::expanded::merge`], and this one is not reachable from
//! there: two `@json` nodes are leaves to JSON-LD and would replace one another
//! wholesale.
//!
//! Clause 5.5.12 says otherwise. The merge patch "merges the provided
//! information with the existing information up to an arbitrary depth, e.g.
//! including going into JSON objects representing a Property value", and clause
//! 4.5.24.2 makes `json` exactly such a value — it prohibits `value` on the
//! family "as value is a generalization of json". Clause 5.5.4 confirms the
//! reading from the other side: its third prohibition names
//! `urn:ngsi-ld:null` "as the value of a key value pair within a JSON object,
//! which is the right-hand side of the value of a Property" and excepts merge
//! operations from it alone, which would be pointless if a merge could not
//! reach inside such an object.
//!
//! The deletion token inside a JSON literal is the NGSI-LD Null *string*, never
//! a JSON `null`. Clause 4.5.0 introduces the URI precisely because "null
//! cannot be used as a value in NGSI-LD", and inside a `@json` literal a JSON
//! `null` survives expansion untouched — so it is ordinary data, and a patch
//! that carries one writes it rather than deleting the key.

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
        match target_members.get_mut(key) {
            Some(existing) if existing.is_object() && patched.is_object() => merge_json_literal(existing, patched),
            // clone: both the key and its value are borrowed from the patch.
            Some(_) | None => {
                target_members.insert(key.clone(), patched.clone());
            }
        }
    }
}

/// Whether some key inside a raw JSON value holds the NGSI-LD Null.
///
/// This is clause 5.5.4's third prohibition, which names the sentinel "as the
/// value of a key value pair within a JSON object" and excepts merge operations
/// from it alone. Two things follow from that wording and are both load-bearing
/// here. The sentinel is a bare string rather than the expanded node the other
/// two prohibitions are written against, because expansion does not descend
/// into a `@json` value. And the raw value itself is never examined: a `json`
/// member that *is* the sentinel deletes the whole `JsonProperty`, which is the
/// second prohibition — the one a partial update is also excepted from.
#[must_use]
pub fn json_object_carries_null_sentinel(value: &Value) -> bool {
    match value {
        Value::Object(members) => members.values().any(is_or_contains_null_sentinel),
        Value::Array(items) => items.iter().any(json_object_carries_null_sentinel),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}

/// Whether a value held under some key is the NGSI-LD Null, or nests one.
fn is_or_contains_null_sentinel(value: &Value) -> bool {
    match value {
        Value::String(text) => text == NULL_URI.as_str(),
        Value::Array(items) => items.iter().any(is_or_contains_null_sentinel),
        Value::Object(members) => members.values().any(is_or_contains_null_sentinel),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::expanded::json_literal_merge::{json_object_carries_null_sentinel, merge_json_literal};
    use serde_json::json;

    /// The fixture of ETSI test `056_10_01`: a JSON object holding a genuine JSON
    /// `null` alongside ordinary members.
    fn stored_json() -> serde_json::Value {
        json!({"id": "some-id", "type": "some-type", "null": null, "anythingElse": "anything-else"})
    }

    /// Clause 5.5.12 removes the key the patch set to the NGSI-LD Null and
    /// leaves every other key of the stored object alone — including the one
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

    /// Clause 5.5.4's third prohibition is detected on the raw value, at any
    /// depth and through arrays, because expansion leaves a JSON literal
    /// untouched.
    #[test]
    fn a_sentinel_under_some_key_is_found_at_any_depth() {
        assert!(json_object_carries_null_sentinel(&json!({"a": {"b": ["urn:ngsi-ld:null"]}})));
        assert!(json_object_carries_null_sentinel(&json!([{"a": "urn:ngsi-ld:null"}])));
        assert!(!json_object_carries_null_sentinel(&stored_json()));
        assert!(!json_object_carries_null_sentinel(&json!({"a": null})));
    }

    /// A `json` member that *is* the sentinel deletes the whole
    /// `JsonProperty`, which is clause 5.5.4's second prohibition and is
    /// excepted for a partial update as well as for a merge. It must not be
    /// reported as the third.
    #[test]
    fn the_whole_value_being_the_sentinel_is_not_the_nested_prohibition() {
        assert!(!json_object_carries_null_sentinel(&json!("urn:ngsi-ld:null")));
        assert!(!json_object_carries_null_sentinel(&json!(["urn:ngsi-ld:null"])));
    }
}
