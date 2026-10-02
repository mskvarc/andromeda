//! The two representations clause 4.5.22 gives a `ListRelationship`'s
//! `objectList`, and the reading that accepts either.
//!
//! Clause 4.5.22.2, Mandatory: "`objectList`: a JSON representation of an
//! ordered array of Relationship Objects each consisting of a JSON object
//! containing a single Attribute with a key called `object` and its value
//! holding the Relationship's object represented by a URI." Clause 4.5.22.3
//! admits the bare URI string beside that wrapper, and Table 5.2.37-1 splits
//! the two forms: "In the normalized form, each array element holds a JSON
//! object containing a single Attribute with a key called `object` and where
//! the value is a valid URI. In the concise form, each string in the array
//! holds a valid URI." Clause 4.5.4 gives the simplified rendering "an ordered
//! array holding the `ListRelationship`'s Objects (represented as URIs)", which
//! is the concise array again.
//!
//! Every renderer that has to move between those forms reads them from here, so
//! a query response, a notification and a temporal evolution cannot come to
//! disagree about which shape a representation gives the member.
//! `previousObjectList` is governed by the same rules: clause 4.5.22.2 makes
//! its representation "the same as that of `objectList`".
//!
//! The normalized representation itself is not defined here. It belongs to the
//! member, so it lives with the member, in
//! [`andromeda_ngsi_ld_model::attribute::object_list`]: the typed
//! `RelationshipObjectList` renders through the same function this module's walk
//! calls, and two implementations of it disagreed about clause 4.5.22.2's
//! single-NGSI-LD-Null deletion encoding.

use andromeda_ngsi_ld_model::attribute::object_list::normalized_object_list;
use andromeda_ngsi_ld_vocabulary::{
    compact::{classes as compact_classes, properties as compact_properties},
    is_core_term,
};
use serde_json::{Map, Value};
use std::{borrow::Cow, slice::from_ref};

/// The bare-URI form of one `objectList` value.
///
/// Table 5.2.37-1 gives the concise form "each string in the array holds a
/// valid URI", and clause 4.5.4 gives the simplified rendering the same ordered
/// array of URIs, so both renderings collapse the member through here.
#[must_use]
pub fn bare_uri_object_list(value: Value) -> Value {
    match value {
        Value::Array(members) => Value::Array(members.into_iter().map(bare_uri_member).collect()),
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => other,
    }
}

/// The URI one rendered `objectList` member names, in either representation.
///
/// Returns `None` for a member that names no URI at all; a caller decides
/// whether that is a value to skip or a document to refuse.
#[must_use]
pub fn object_list_member_target(member: &Value) -> Option<&str> {
    member.as_str().or_else(|| member.get(compact_properties::OBJECT).and_then(Value::as_str))
}

/// A member the broker carries verbatim, every `objectList` and
/// `previousObjectList` of its `ListRelationship` instances -- and of their
/// sub-attributes -- in the clause 4.5.22.2 normalized form.
///
/// A typed `ListRelationship` is written in that form by its own member; a
/// member carried verbatim keeps whichever spelling its Context Producer wrote
/// unless the representation is imposed here. Only an instance that declares
/// itself a `ListRelationship` is rewritten, and only the attribute structure
/// is walked, so a client's value that happens to carry a member of the same
/// name is left as its writer stored it.
pub(crate) fn normalized_verbatim_member(member: Cow<'_, Value>) -> Cow<'_, Value> {
    if !holds_list_relationship(&member) {
        return member;
    }
    let mut normalized = member.into_owned();
    normalize_member(&mut normalized);
    Cow::Owned(normalized)
}

/// Whether a verbatim member holds a `ListRelationship` instance anywhere in
/// its attribute structure.
fn holds_list_relationship(member: &Value) -> bool {
    instances(member)
        .any(|instance| declares_list_relationship(instance) || instance.iter().any(|(name, nested)| !is_core_term(name) && holds_list_relationship(nested)))
}

/// Normalize the lists of every `ListRelationship` instance of a member.
fn normalize_member(member: &mut Value) {
    let instances: Vec<&mut Map<String, Value>> = match member {
        Value::Object(instance) => vec![instance],
        Value::Array(items) => items.iter_mut().filter_map(Value::as_object_mut).collect(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Vec::new(),
    };
    for instance in instances {
        if declares_list_relationship(instance) {
            for name in [compact_properties::OBJECT_LIST, compact_properties::PREVIOUS_OBJECT_LIST] {
                if let Some(list) = instance.get_mut(name) {
                    *list = normalized_object_list(list.take());
                }
            }
        }
        instance
            .iter_mut()
            .filter(|(name, _)| !is_core_term(name))
            .for_each(|(_, nested)| normalize_member(nested));
    }
}

/// The instances one member holds: the one object, or each object of an array.
fn instances(member: &Value) -> impl Iterator<Item = &Map<String, Value>> {
    let items = match member {
        Value::Array(items) => items.as_slice(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => from_ref(member),
    };
    items.iter().filter_map(Value::as_object)
}

/// Whether an instance declares itself a `ListRelationship`.
fn declares_list_relationship(instance: &Map<String, Value>) -> bool {
    instance.get("type").and_then(Value::as_str) == Some(compact_classes::LIST_RELATIONSHIP)
}

/// The URI one member carries, as the bare string the concise and simplified
/// renderings hold.
///
/// A member that is neither spelling passes through: clause 4.5.4 has no place
/// to report a malformed member, and dropping it would silently shorten the
/// ordered list.
fn bare_uri_member(member: Value) -> Value {
    match member {
        Value::Object(mut fields) => match fields.remove(compact_properties::OBJECT) {
            Some(target) => target,
            None => Value::Object(fields),
        },
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_)) => other,
    }
}

#[cfg(test)]
mod tests {
    use crate::object_list::{bare_uri_object_list, normalized_verbatim_member, object_list_member_target};
    use serde_json::{Value, json};
    use std::borrow::Cow;

    /// The member as the normalization writes it.
    fn normalized(member: Value) -> Value {
        normalized_verbatim_member(Cow::Owned(member)).into_owned()
    }

    /// Table 5.2.37-1: "In the concise form, each string in the array holds a
    /// valid URI", and clause 4.5.4 gives the simplified rendering the same
    /// ordered array of URIs.
    #[test]
    fn the_bare_uri_form_unwraps_each_relationship_object() {
        pretty_assertions::assert_eq!(
            bare_uri_object_list(json!([{"object": "urn:ngsi-ld:A:1"}, "urn:ngsi-ld:A:2"])),
            json!(["urn:ngsi-ld:A:1", "urn:ngsi-ld:A:2"])
        );
    }

    /// Clause 4.5.22.2 and clause 4.5.22.3 both name the same targets, so a
    /// reader accepts either spelling.
    #[test]
    fn a_member_names_its_target_in_either_representation() {
        pretty_assertions::assert_eq!(object_list_member_target(&json!("urn:ngsi-ld:A:1")), Some("urn:ngsi-ld:A:1"));
        pretty_assertions::assert_eq!(object_list_member_target(&json!({"object": "urn:ngsi-ld:A:1"})), Some("urn:ngsi-ld:A:1"));
        pretty_assertions::assert_eq!(object_list_member_target(&json!(7)), None);
    }

    /// Clause 4.5.22.2 makes `previousObjectList`'s representation "the same as
    /// that of `objectList`", so both are normalized.
    #[test]
    fn both_list_members_of_a_list_relationship_are_normalized() {
        let member = normalized(json!({
            "type": "ListRelationship",
            "objectList": ["urn:ngsi-ld:Room:3"],
            "previousObjectList": ["urn:ngsi-ld:Room:1", "urn:ngsi-ld:Room:2"]
        }));
        pretty_assertions::assert_eq!(member["objectList"], json!([{"object": "urn:ngsi-ld:Room:3"}]));
        pretty_assertions::assert_eq!(
            member["previousObjectList"],
            json!([{"object": "urn:ngsi-ld:Room:1"}, {"object": "urn:ngsi-ld:Room:2"}])
        );
    }

    /// Clause 4.5.2.2 prohibits `objectList` on a Property, so a member of that
    /// name inside a client's value is the producer's own data and is left
    /// exactly as it was stored, and a member that holds no `ListRelationship`
    /// is not copied at all.
    #[test]
    fn a_client_value_shaped_like_a_list_relationship_is_left_untouched() {
        for member in [
            json!({"type": "JsonProperty", "json": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:R:1"]}}),
            json!({"type": "Property", "value": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:R:1"]}}),
        ] {
            assert!(matches!(normalized_verbatim_member(Cow::Borrowed(&member)), Cow::Borrowed(_)), "{member}");
        }
    }

    /// Every instance of a multi-attribute (clause 4.5.5), and a sub-attribute,
    /// is reached.
    #[test]
    fn every_instance_and_sub_attribute_is_reached() {
        let member = normalized(json!([
            {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:1"]},
            {"type": "Property", "value": 1, "rooms": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:2"]}}
        ]));
        pretty_assertions::assert_eq!(member[0]["objectList"], json!([{"object": "urn:ngsi-ld:Room:1"}]));
        pretty_assertions::assert_eq!(member[1]["rooms"]["objectList"], json!([{"object": "urn:ngsi-ld:Room:2"}]));
    }
}
