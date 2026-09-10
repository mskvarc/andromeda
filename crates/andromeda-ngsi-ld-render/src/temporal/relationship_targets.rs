//! The identifiers a rendered Relationship member names.
//!
//! Clause 4.5.23 follows a Relationship to its target Entity, and the member it
//! reads is written in whatever shape the family and the representation give
//! it: `object` is "a URI or array of URIs" (clause 4.5.3.2), while
//! `objectList` is an ordered array whose members are `object`-keyed JSON
//! objects in the normalized form and bare URI strings in the concise one
//! (clause 4.5.22.2, clause 4.5.22.3 and Table 5.2.37-1). Reading a target out
//! of any of them is one rule, so it lives in one place rather than once per
//! traversal that needs it.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde_json::Value;

/// The identifiers one rendered `object` or `objectList` value names, in order.
///
/// A value that is not a URI at all is skipped rather than rejected: clause
/// 4.5.23 degrades an unusable reference instead of failing the retrieval.
#[must_use]
pub fn rendered_relationship_targets(value: &Value) -> Vec<Uri> {
    match value {
        Value::String(raw) => raw.parse().into_iter().collect(),
        Value::Array(members) => members.iter().filter_map(member_target).collect(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => Vec::new(),
    }
}

/// The identifier one member of a rendered array names.
///
/// Both spellings clause 4.5.22 gives an `objectList` member are read, and the
/// bare string doubles as one element of the array of URIs clause 4.5.3.2 gives
/// a multi-target `object`.
fn member_target(member: &Value) -> Option<Uri> {
    member
        .as_str()
        .or_else(|| member.get(compact_properties::OBJECT).and_then(Value::as_str))
        .and_then(|raw| raw.parse().ok())
}

#[cfg(test)]
mod tests {
    use crate::temporal::relationship_targets::rendered_relationship_targets;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use serde_json::json;

    fn uri(raw: &str) -> Uri {
        raw.parse().expect("a valid URI")
    }

    /// Clause 4.5.3.2: `object` is "the Relationship's object represented by a
    /// URI or array of URIs", so both shapes name their targets.
    #[test]
    fn a_relationship_object_names_one_target_or_an_array_of_them() {
        pretty_assertions::assert_eq!(rendered_relationship_targets(&json!("urn:ngsi-ld:T:B")), vec![uri("urn:ngsi-ld:T:B")]);
        pretty_assertions::assert_eq!(
            rendered_relationship_targets(&json!(["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"])),
            vec![uri("urn:ngsi-ld:T:B"), uri("urn:ngsi-ld:T:C")]
        );
    }

    /// Table 5.2.37-1: a normalized `objectList` member "holds a JSON object
    /// containing a single Attribute with a key called `object`", a concise one
    /// "holds a valid URI", and both name the same target in the same order.
    #[test]
    fn both_object_list_representations_name_the_same_targets_in_order() {
        let normalized = json!([{"object": "urn:ngsi-ld:T:B"}, {"object": "urn:ngsi-ld:T:C"}]);
        let concise = json!(["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]);
        let expected = vec![uri("urn:ngsi-ld:T:B"), uri("urn:ngsi-ld:T:C")];
        pretty_assertions::assert_eq!(rendered_relationship_targets(&normalized), expected);
        pretty_assertions::assert_eq!(rendered_relationship_targets(&concise), expected);
    }

    /// Clause 4.5.23 degrades an unusable reference rather than failing the
    /// retrieval, so a member that names no URI contributes nothing and the
    /// members beside it still resolve.
    #[test]
    fn a_member_that_names_no_uri_is_skipped_rather_than_rejected() {
        pretty_assertions::assert_eq!(rendered_relationship_targets(&json!("not a uri")), Vec::<Uri>::new());
        pretty_assertions::assert_eq!(rendered_relationship_targets(&json!(42)), Vec::<Uri>::new());
        pretty_assertions::assert_eq!(rendered_relationship_targets(&json!({"nested": "object"})), Vec::<Uri>::new());
        pretty_assertions::assert_eq!(
            rendered_relationship_targets(&json!(["urn:ngsi-ld:T:B", 7, {"unrelated": 1}])),
            vec![uri("urn:ngsi-ld:T:B")]
        );
    }
}
