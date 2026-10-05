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
//! [`andromeda_ngsi_ld_model::attribute::object_list`].

use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde_json::Value;

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
    use crate::object_list::{bare_uri_object_list, object_list_member_target};
    use serde_json::json;

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
}
