//! The `objectList` / `previousObjectList` member (ETSI GS CIM 009 clause 5.2.37).

use andromeda_ngsi_ld_lexicon::uri::{Uri, UriParseError};
use andromeda_ngsi_ld_vocabulary::{NULL_IRI, NULL_URI, compact::properties as compact_properties};
use derive_more::{AsRef, Deref, From};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeError};
use serde_json::{Map, Value};
use std::{slice::Iter, str::FromStr, vec::IntoIter};
use thiserror::Error;

/// `objectList` / `previousObjectList` value as defined by clause 5.2.37.
///
/// The domain value is the ordered list of target identifiers, which is what
/// Table 5.2.37-1 calls an "Ordered array of Relationship target objects". The
/// two wire spellings clause 4.5.22 gives the member are a representation
/// decision, and both halves of it are written out below: the model is the
/// normalized representation, so that is what it serialises as, while
/// deserialisation accepts either spelling because clause 4.5.22.3 lets a
/// Context Producer write the concise one.
#[derive(Debug, Clone, Default, PartialEq, Eq, AsRef, Deref, From)]
pub struct RelationshipObjectList(pub Vec<Uri>);

/// The clause 4.5.22.2 normalized representation of one `objectList` value.
///
/// Clause 4.5.22.2, Mandatory: "`objectList`: a JSON representation of an
/// ordered array of Relationship Objects each consisting of a JSON object
/// containing a single Attribute with a key called `object` and its value
/// holding the Relationship's object represented by a URI." Table 5.2.37-1
/// confines the bare-string spelling to the concise form, and clause 4.5.22.3
/// lets a Context Producer write it, so a member already in either spelling
/// reaches the normalized one from here and the rendering is idempotent.
///
/// The clause gives the member a second right-hand side beside the ordered
/// array of Relationship Objects: "An array consisting of a single NGSI-LD Null
/// … can be used as the right-hand side of the `objectList` … as well as in
/// notifications and in temporal evolutions (for encoding a deleted
/// `ListRelationship`)." An NGSI-LD Null is not a Relationship Object, so that
/// array keeps the single-element spelling clause 4.5.0 gives it rather than
/// being wrapped as one.
///
/// This is the one implementation of the representation. The typed member below
/// renders through it and so does every renderer that has to move a payload
/// between the two spellings, which is what keeps a query response, a
/// notification and a temporal evolution from disagreeing about which shape the
/// member has. `previousObjectList` is governed by the same rules: clause
/// 4.5.22.2 makes its representation "the same as that of `objectList`".
#[must_use]
pub fn normalized_object_list(value: Value) -> Value {
    match value {
        Value::Array(members) => {
            if is_deletion_encoding(&members) {
                Value::Array(members)
            } else {
                Value::Array(members.into_iter().map(normalized_member).collect())
            }
        }
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => other,
    }
}

/// One member of a normalized `objectList`, built from whichever spelling the
/// value arrived in.
fn normalized_member(member: Value) -> Value {
    match member {
        Value::String(uri) => {
            let mut object = Map::with_capacity(1);
            object.insert(compact_properties::OBJECT.to_owned(), Value::String(uri));
            Value::Object(object)
        }
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_)) => other,
    }
}

/// Whether an `objectList` array encodes a deletion rather than a list of
/// Relationship Objects (clause 4.5.22.2).
fn is_deletion_encoding(members: &[Value]) -> bool {
    let [Value::String(only)] = members else {
        return false;
    };
    only.as_str() == NULL_URI.as_str() || only.as_str() == NULL_IRI.as_str()
}

// Hand-written because the serialised shape is not the shape of the field and
// no derive expresses it: Table 5.2.37-1 splits the two representations --
// "In the normalized form, each array element holds a JSON object containing a
// single Attribute with a key called `object` and where the value is a valid
// URI. In the concise form, each string in the array holds a valid URI" -- and
// the model carries the normalized one, which the concise and simplified
// renderings are derived from at the presentation boundary. A `#[serde(into =
// ...)]` derive would express it only by cloning the whole list first.
impl Serialize for RelationshipObjectList {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // The ordered targets are written out as the bare array first and handed
        // to the single normalizer above, rather than wrapped here: two spellings
        // of the representation are what let one path honour clause 4.5.22.2's
        // deletion encoding while the other wrapped the NGSI-LD Null as though it
        // were a Relationship Object. The targets are copied into owned JSON
        // strings because the normalizer reads the member as the document it is
        // about to become.
        let bare = Value::Array(self.0.iter().map(|object| Value::String(object.as_str().to_owned())).collect());
        normalized_object_list(bare).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RelationshipObjectList {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<RelationshipObjectList, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let items = value
            .as_array()
            .ok_or_else(|| DeError::custom("objectList must be an array"))?
            .iter()
            .map(parse_relationship_object_entry)
            .collect::<Result<Vec<_>, _>>()
            .map_err(DeError::custom)?;
        Ok(RelationshipObjectList(items))
    }
}

impl<'a> IntoIterator for &'a RelationshipObjectList {
    type IntoIter = Iter<'a, Uri>;
    type Item = &'a Uri;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl IntoIterator for RelationshipObjectList {
    type IntoIter = IntoIter<Uri>;
    type Item = Uri;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// Failures parsing one entry of an NGSI-LD `objectList` (clause 5.2.37).
#[derive(Debug, Error)]
pub enum RelationshipObjectEntryError {
    /// The entry was neither a URI string nor an `{"object": …}` wrapper.
    #[error("relationship object list entries must be strings or {{\"object\": ...}} objects")]
    NotAnObjectReference,
    /// The entry held a value that is not a valid URI.
    #[error("invalid relationship object URI: {source}")]
    Uri {
        /// Underlying URI parse failure.
        #[source]
        source: UriParseError,
    },
}

/// Read one `objectList` entry, accepting both the bare URI string and the
/// `{"object": "…"}` wrapper clause 5.2.37 permits.
///
/// # Errors
///
/// Returns [`RelationshipObjectEntryError::NotAnObjectReference`] when neither
/// shape matches, or [`RelationshipObjectEntryError::Uri`] when the extracted
/// text is not a URI.
pub fn parse_relationship_object_entry(value: &Value) -> Result<Uri, RelationshipObjectEntryError> {
    let raw = match value.as_str() {
        Some(uri) => uri,
        None => value
            .as_object()
            .and_then(|object| object.get("object"))
            .and_then(Value::as_str)
            .ok_or(RelationshipObjectEntryError::NotAnObjectReference)?,
    };
    Uri::from_str(raw).map_err(|source| RelationshipObjectEntryError::Uri { source })
}

#[cfg(test)]
mod tests {
    use crate::attribute::object_list::{RelationshipObjectList, normalized_object_list};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use serde_json::json;
    use std::str::FromStr;

    /// Clause 4.5.22.2: "An array consisting of a single NGSI-LD Null … can be
    /// used as the right-hand side of the `objectList` … as well as in
    /// notifications and in temporal evolutions (for encoding a deleted
    /// `ListRelationship`)." An NGSI-LD Null is not a Relationship Object, so it
    /// is not wrapped as one — and because the typed member renders through the
    /// same function every payload rewrite uses, it cannot be wrapped on one
    /// path and left alone on the other.
    #[test]
    fn the_deletion_encoding_survives_both_readings_of_the_normalized_representation() {
        pretty_assertions::assert_eq!(normalized_object_list(json!(["urn:ngsi-ld:null"])), json!(["urn:ngsi-ld:null"]));
        pretty_assertions::assert_eq!(
            normalized_object_list(json!(["https://uri.etsi.org/ngsi-ld/null"])),
            json!(["https://uri.etsi.org/ngsi-ld/null"])
        );

        let deleted = RelationshipObjectList(vec![Uri::from_str("urn:ngsi-ld:null").expect("the NGSI-LD Null is a URN")]);
        pretty_assertions::assert_eq!(serde_json::to_value(&deleted).expect("serialises"), json!(["urn:ngsi-ld:null"]));
    }

    /// The deletion encoding is the array of exactly one NGSI-LD Null, so a list
    /// that merely contains one is still an ordered array of Relationship
    /// Objects.
    #[test]
    fn a_list_that_only_contains_the_null_sentinel_is_still_a_list_of_relationship_objects() {
        let mixed = RelationshipObjectList(vec![
            Uri::from_str("urn:ngsi-ld:null").expect("the NGSI-LD Null is a URN"),
            Uri::from_str("urn:ngsi-ld:Entity:1").expect("uri"),
        ]);
        pretty_assertions::assert_eq!(
            serde_json::to_value(&mixed).expect("serialises"),
            json!([{"object": "urn:ngsi-ld:null"}, {"object": "urn:ngsi-ld:Entity:1"}])
        );
    }

    /// The normalized rendering of an already-normalized list is itself, so the
    /// shape a Context Producer wrote does not decide the shape it reads back.
    #[test]
    fn the_normalized_representation_is_idempotent() {
        let normalized = json!([{"object": "urn:ngsi-ld:A:1"}]);
        // clone: the transformation consumes its input and the expectation keeps the original
        pretty_assertions::assert_eq!(normalized_object_list(normalized.clone()), normalized);
        pretty_assertions::assert_eq!(
            normalized_object_list(json!(["urn:ngsi-ld:A:1", "urn:ngsi-ld:A:2"])),
            json!([{"object": "urn:ngsi-ld:A:1"}, {"object": "urn:ngsi-ld:A:2"}])
        );
    }

    #[test]
    fn relationship_object_list_deserializes_string_entries() {
        let value: RelationshipObjectList = serde_json::from_value(serde_json::json!(["urn:ngsi-ld:Entity:1"])).expect("objectList");
        pretty_assertions::assert_eq!(value.0.len(), 1);
    }

    #[test]
    fn relationship_object_list_deserializes_object_entries() {
        let value: RelationshipObjectList = serde_json::from_value(serde_json::json!([{ "object": "urn:ngsi-ld:Entity:1" }])).expect("objectList");
        pretty_assertions::assert_eq!(value.0.len(), 1);
    }

    /// Clause 4.5.22.2, Mandatory: an `objectList` is "an ordered array of
    /// Relationship Objects each consisting of a JSON object containing a
    /// single Attribute with a key called `object` and its value holding the
    /// Relationship's object represented by a URI", and Table 5.2.37-1 confines
    /// the bare-string spelling to the concise form. The model carries the
    /// normalized representation, so that is the array it writes.
    #[test]
    fn relationship_object_list_serialises_the_normalized_relationship_objects() {
        let list = RelationshipObjectList(vec![
            Uri::from_str("urn:ngsi-ld:Entity:1").expect("uri a"),
            Uri::from_str("urn:ngsi-ld:Entity:2").expect("uri b"),
        ]);
        pretty_assertions::assert_eq!(
            serde_json::to_value(&list).expect("serialises"),
            serde_json::json!([{"object": "urn:ngsi-ld:Entity:1"}, {"object": "urn:ngsi-ld:Entity:2"}])
        );
    }

    /// Both spellings clause 4.5.22 gives the member name the same targets, so
    /// either one a Context Producer writes reads back as the normalized array.
    #[test]
    fn both_written_representations_serialise_back_to_the_normalized_one() {
        let concise: RelationshipObjectList =
            serde_json::from_value(serde_json::json!(["urn:ngsi-ld:Entity:1", "urn:ngsi-ld:Entity:2"])).expect("concise objectList");
        let normalized: RelationshipObjectList =
            serde_json::from_value(serde_json::json!([{"object": "urn:ngsi-ld:Entity:1"}, {"object": "urn:ngsi-ld:Entity:2"}])).expect("normalized objectList");
        pretty_assertions::assert_eq!(concise, normalized);
        pretty_assertions::assert_eq!(
            serde_json::to_value(&concise).expect("serialises"),
            serde_json::json!([{"object": "urn:ngsi-ld:Entity:1"}, {"object": "urn:ngsi-ld:Entity:2"}])
        );
    }
}
