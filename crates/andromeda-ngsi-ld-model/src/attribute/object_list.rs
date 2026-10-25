//! The `objectList` / `previousObjectList` member (ETSI GS CIM 009 clause 5.2.37).

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_vocabulary::ngsi_ld_null::is_ngsi_ld_null;
use derive_more::{AsRef, Deref, From};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{slice::Iter, vec::IntoIter};

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

/// The clause 4.5.22.2 normalized representation of the ordered targets of one
/// `objectList` value, written straight from the targets.
///
/// Clause 4.5.22.2, Mandatory: "`objectList`: a JSON representation of an
/// ordered array of Relationship Objects each consisting of a JSON object
/// containing a single Attribute with a key called `object` and its value
/// holding the Relationship's object represented by a URI." Table 5.2.37-1
/// confines the bare-string spelling to the concise form.
///
/// The clause gives the member a second right-hand side beside the ordered
/// array of Relationship Objects: "An array consisting of a single NGSI-LD Null
/// … can be used as the right-hand side of the `objectList` … as well as in
/// notifications and in temporal evolutions (for encoding a deleted
/// `ListRelationship`)." An NGSI-LD Null is not a Relationship Object, so that
/// array keeps the single-element spelling clause 4.5.0 gives it rather than
/// being wrapped as one.
///
/// The typed member below writes through it, so a query response, a
/// notification and a temporal evolution agree on which shape the member has.
/// `previousObjectList` is governed by the same rules: clause 4.5.22.2 makes
/// its representation "the same as that of `objectList`".
#[derive(Debug, Clone, Copy)]
struct NormalizedObjectList<'a, T> {
    /// The ordered targets.
    targets: &'a [T],
}

impl<'a, T: AsRef<str>> NormalizedObjectList<'a, T> {
    /// The normalized representation of `targets`.
    const fn new(targets: &'a [T]) -> NormalizedObjectList<'a, T> {
        NormalizedObjectList { targets }
    }

    /// Whether the targets encode a deletion rather than a list of
    /// Relationship Objects (clause 4.5.22.2).
    fn is_deletion_encoding(&self) -> bool {
        matches!(self.targets, [only] if is_ngsi_ld_null(only.as_ref()))
    }
}

impl<T: AsRef<str>> Serialize for NormalizedObjectList<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.is_deletion_encoding() {
            serializer.collect_seq(self.targets.iter().map(AsRef::as_ref))
        } else {
            serializer.collect_seq(self.targets.iter().map(|target| RelationshipObject { object: target.as_ref() }))
        }
    }
}

/// One Relationship Object of a normalized `objectList` (clause 4.5.22.2).
#[derive(Serialize)]
struct RelationshipObject<'a> {
    /// The Relationship's object, a URI.
    object: &'a str,
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
        NormalizedObjectList::new(&self.0).serialize(serializer)
    }
}

// Hand-written: each entry is read in whichever spelling clause 4.5.22 gives
// it, and the list holds the targets alone.
impl<'de> Deserialize<'de> for RelationshipObjectList {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<RelationshipObjectList, D::Error> {
        Vec::<ObjectListEntry>::deserialize(deserializer).map(|entries| RelationshipObjectList(entries.into_iter().map(ObjectListEntry::into_target).collect()))
    }
}

/// One entry of an `objectList` as written: the bare URI of the concise form,
/// the Relationship Object of the normalized form (clause 4.5.22.2), or the
/// node reference a concise entry was expanded to and compacted back as.
#[derive(Deserialize)]
#[serde(
    untagged,
    expecting = "a relationship target URI, an {\"object\": ...} object or an {\"id\": ...} node reference"
)]
enum ObjectListEntry {
    /// The bare URI.
    Bare(Uri),
    /// The Relationship Object.
    Object {
        /// The target.
        object: Uri,
    },
    /// The node reference.
    Node {
        /// The target.
        id: Uri,
    },
}

impl ObjectListEntry {
    /// The target the entry names.
    fn into_target(self) -> Uri {
        match self {
            ObjectListEntry::Bare(target) | ObjectListEntry::Object { object: target } | ObjectListEntry::Node { id: target } => target,
        }
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

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::attribute::object_list::{NormalizedObjectList, RelationshipObjectList};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use serde_json::json;
    use std::str::FromStr;

    /// The normalized representation of `targets`, as JSON.
    fn normalized(targets: &[&str]) -> serde_json::Value {
        serde_json::to_value(NormalizedObjectList::new(targets)).expect("serialises")
    }

    /// Clause 4.5.22.2: "An array consisting of a single NGSI-LD Null … can be
    /// used as the right-hand side of the `objectList` … as well as in
    /// notifications and in temporal evolutions (for encoding a deleted
    /// `ListRelationship`)." An NGSI-LD Null is not a Relationship Object, so it
    /// is not wrapped as one — and because the typed member renders through the
    /// same function every payload rewrite uses, it cannot be wrapped on one
    /// path and left alone on the other.
    #[test]
    fn the_deletion_encoding_survives_both_readings_of_the_normalized_representation() {
        pretty_assertions::assert_eq!(normalized(&["urn:ngsi-ld:null"]), json!(["urn:ngsi-ld:null"]));
        pretty_assertions::assert_eq!(
            normalized(&["https://uri.etsi.org/ngsi-ld/null"]),
            json!([{"object": "https://uri.etsi.org/ngsi-ld/null"}]),
            "clause 3.1 names the URN alone, so the IRI is a target like any other"
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

    /// The targets of a list of any length other than one are each wrapped as a
    /// Relationship Object, the empty list included.
    #[test]
    fn the_normalized_representation_wraps_every_target() {
        pretty_assertions::assert_eq!(
            normalized(&["urn:ngsi-ld:A:1", "urn:ngsi-ld:A:2"]),
            json!([{"object": "urn:ngsi-ld:A:1"}, {"object": "urn:ngsi-ld:A:2"}])
        );
        pretty_assertions::assert_eq!(normalized(&["urn:ngsi-ld:A:1"]), json!([{"object": "urn:ngsi-ld:A:1"}]));
        pretty_assertions::assert_eq!(normalized(&[]), json!([]));
    }

    #[test]
    fn relationship_object_list_deserializes_string_entries() {
        let value: RelationshipObjectList = serde_json::from_value(serde_json::json!(["urn:ngsi-ld:Entity:1"])).expect("objectList");
        pretty_assertions::assert_eq!(value.0.len(), 1);
    }

    /// A concise entry expanded to a node reference and compacted back reads
    /// as the target it names.
    #[test]
    fn relationship_object_list_deserializes_node_reference_entries() {
        let value: RelationshipObjectList =
            serde_json::from_value(serde_json::json!([{"id": "urn:ngsi-ld:Entity:1"}, "urn:ngsi-ld:Entity:2"])).expect("objectList");
        pretty_assertions::assert_eq!(value.0.len(), 2);
        assert!(serde_json::from_value::<RelationshipObjectList>(serde_json::json!([{"subject": "urn:ngsi-ld:Entity:1"}])).is_err());
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
