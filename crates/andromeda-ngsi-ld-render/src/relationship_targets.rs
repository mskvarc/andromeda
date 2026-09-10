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

use crate::object_list::object_list_member_target;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use serde_json::Value;

/// Which of the two spellings clause 4.5.3.2 admits a rendered `object` uses.
///
/// The distinction survives the reading of the targets because clause 4.5.23.2
/// nests the Linked Entities under a member that mirrors it: a Relationship
/// naming one target answers with one `entity` object, one naming an array of
/// targets answers with an array of them. Deriving the answer from how many
/// targets happened to resolve would turn a one-element array of targets into a
/// bare object and change the shape a client reads back.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ObjectSpelling {
    /// `object` is a single URI written as a bare string.
    OneUri,
    /// `object` is an array of URIs, whatever its length.
    ArrayOfUris,
    /// `object` is neither, so there is nothing to follow.
    NoUri,
}

impl ObjectSpelling {
    /// Read the spelling of one rendered `object` member.
    #[must_use]
    pub const fn of(value: &Value) -> ObjectSpelling {
        match value {
            Value::String(_) => ObjectSpelling::OneUri,
            Value::Array(_) => ObjectSpelling::ArrayOfUris,
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => ObjectSpelling::NoUri,
        }
    }
}

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
    object_list_member_target(member).and_then(|raw| raw.parse().ok())
}

#[cfg(test)]
mod tests {
    use crate::relationship_targets::{ObjectSpelling, rendered_relationship_targets};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use serde_json::json;

    /// Clause 4.5.3.2's two spellings are told apart by the JSON shape, not by
    /// how many targets the array happens to hold.
    #[test]
    fn the_two_object_spellings_are_told_apart_by_shape_and_not_by_length() {
        pretty_assertions::assert_eq!(ObjectSpelling::of(&json!("urn:ngsi-ld:T:B")), ObjectSpelling::OneUri);
        pretty_assertions::assert_eq!(ObjectSpelling::of(&json!(["urn:ngsi-ld:T:B"])), ObjectSpelling::ArrayOfUris);
        pretty_assertions::assert_eq!(ObjectSpelling::of(&json!(["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"])), ObjectSpelling::ArrayOfUris);
        pretty_assertions::assert_eq!(ObjectSpelling::of(&json!({"object": "urn:ngsi-ld:T:B"})), ObjectSpelling::NoUri);
        pretty_assertions::assert_eq!(ObjectSpelling::of(&json!(null)), ObjectSpelling::NoUri);
    }

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
