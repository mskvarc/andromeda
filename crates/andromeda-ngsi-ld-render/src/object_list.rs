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
use andromeda_ngsi_ld_vocabulary::compact::{classes as compact_classes, properties as compact_properties};
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

/// Rewrite every `objectList` and `previousObjectList` a rendered payload
/// carries into the clause 4.5.22.2 normalized form.
///
/// The walk exists for the payloads that reach the wire as JSON rather than as
/// a typed resource — a notification is compacted from the stored expanded
/// document, so the member keeps whichever spelling its Context Producer wrote
/// unless the representation is imposed here. Only an instance that declares
/// itself a `ListRelationship` is rewritten, so a `JsonProperty` document that
/// happens to carry a member of the same name is left as its writer stored it.
pub fn normalize_object_lists(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            if fields.get("type").and_then(Value::as_str) == Some(compact_classes::LIST_RELATIONSHIP) {
                for member in [compact_properties::OBJECT_LIST, compact_properties::PREVIOUS_OBJECT_LIST] {
                    if let Some(list) = fields.get_mut(member) {
                        *list = normalized_object_list(list.take());
                    }
                }
            }
            for nested in fields.values_mut() {
                normalize_object_lists(nested);
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_object_lists(item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
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
    use crate::object_list::{bare_uri_object_list, normalize_object_lists, object_list_member_target};
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

    /// Clause 4.5.22.2 makes `previousObjectList`'s representation "the same as
    /// that of `objectList`", so a `showChanges` notification normalizes both.
    #[test]
    fn the_walk_normalizes_both_list_members_of_a_list_relationship() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "hasRooms": {
                "type": "ListRelationship",
                "objectList": ["urn:ngsi-ld:Room:3"],
                "previousObjectList": ["urn:ngsi-ld:Room:1", "urn:ngsi-ld:Room:2"]
            }
        });
        normalize_object_lists(&mut payload);
        pretty_assertions::assert_eq!(payload["hasRooms"]["objectList"], json!([{"object": "urn:ngsi-ld:Room:3"}]));
        pretty_assertions::assert_eq!(
            payload["hasRooms"]["previousObjectList"],
            json!([{"object": "urn:ngsi-ld:Room:1"}, {"object": "urn:ngsi-ld:Room:2"}])
        );
    }

    /// Clause 4.5.2.2 prohibits `objectList` on a Property, so a member of that
    /// name inside a `JsonProperty` document is the producer's own data and is
    /// left exactly as it was stored.
    #[test]
    fn the_walk_leaves_a_json_property_document_untouched() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Building:2",
            "type": "Building",
            "config": {"type": "JsonProperty", "json": {"objectList": ["not-a-relationship"]}}
        });
        normalize_object_lists(&mut payload);
        pretty_assertions::assert_eq!(payload["config"]["json"]["objectList"], json!(["not-a-relationship"]));
    }

    /// The walk reaches every instance of a multi-instance attribute
    /// (clause 4.5.5), which a rendering carries as a JSON array.
    #[test]
    fn the_walk_reaches_every_instance_of_a_multi_instance_list_relationship() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Building:3",
            "type": "Building",
            "hasRooms": [
                {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:1"]},
                {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:2"], "datasetId": "urn:ngsi-ld:Dataset:1"}
            ]
        });
        normalize_object_lists(&mut payload);
        pretty_assertions::assert_eq!(payload["hasRooms"][0]["objectList"], json!([{"object": "urn:ngsi-ld:Room:1"}]));
        pretty_assertions::assert_eq!(payload["hasRooms"][1]["objectList"], json!([{"object": "urn:ngsi-ld:Room:2"}]));
    }
}
