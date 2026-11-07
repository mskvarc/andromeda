//! The `objectList` / `previousObjectList` member (ETSI GS CIM 009 clause 5.2.37).

use crate::attribute::recorded_elements::recorded_elements;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_representation::value_budget::ValueBudget;
use andromeda_ngsi_ld_vocabulary::ngsi_ld_null::is_ngsi_ld_null;
use derive_more::{AsRef, Deref, Display, From};
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
    de::{DeserializeSeed, Error as DeserializeError, MapAccess, SeqAccess, Visitor, value::StrDeserializer},
};
use std::{fmt, slice::Iter, vec::IntoIter};
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
        RelationshipObjectList::within(None).deserialize(deserializer)
    }
}

impl RelationshipObjectList {
    /// Read an `objectList`, its entries recorded against `budget` when there is
    /// one.
    #[must_use]
    pub const fn within(budget: Option<&ValueBudget>) -> ObjectListWithin<'_> {
        ObjectListWithin(budget)
    }
}

/// Reads an `objectList`, its entries recorded against the budget of values the
/// entity it belongs to may still hold.
#[derive(Debug, Clone, Copy)]
pub struct ObjectListWithin<'b>(Option<&'b ValueBudget>);

impl<'de> DeserializeSeed<'de> for ObjectListWithin<'_> {
    type Value = RelationshipObjectList;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<RelationshipObjectList, D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for ObjectListWithin<'_> {
    type Value = RelationshipObjectList;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an array of objectList entries")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, entries: A) -> Result<RelationshipObjectList, A::Error> {
        recorded_elements::<ObjectListEntry, A>(entries, self.0).map(|entries| RelationshipObjectList(entries.into_iter().map(|entry| entry.0).collect()))
    }
}

/// The target one entry of an `objectList` names, written as the bare URI of
/// the concise form, the Relationship Object of the normalized form (clause
/// 4.5.22.2), or the node reference a concise entry was expanded to and
/// compacted back as.
struct ObjectListEntry(Uri);

// Hand-written: the entry is read by the kind of JSON it is, so the refusal of
// a target that is no URI, or of an object holding other members, is the
// refusal of the entry. A derived untagged union reports only that no spelling
// matched.
impl<'de> Deserialize<'de> for ObjectListEntry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<ObjectListEntry, D::Error> {
        deserializer.deserialize_any(ObjectListEntryVisitor)
    }
}

/// Reads one `objectList` entry.
struct ObjectListEntryVisitor;

impl<'de> Visitor<'de> for ObjectListEntryVisitor {
    type Value = ObjectListEntry;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a relationship target URI, an {\"object\": ...} object or an {\"id\": ...} node reference")
    }

    fn visit_str<E: DeserializeError>(self, text: &str) -> Result<ObjectListEntry, E> {
        Uri::deserialize(StrDeserializer::<E>::new(text)).map(ObjectListEntry)
    }

    // Clause 4.5.22.2: a Relationship Object is "a JSON object containing a
    // single Attribute with a key called `object`"; a node reference likewise
    // holds its `id` alone.
    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<ObjectListEntry, A::Error> {
        let Some(member) = members.next_key::<EntryMember>()? else {
            return Err(A::Error::custom(ObjectListEntryRefusal::Empty));
        };
        let target = members.next_value::<Uri>()?;
        match members.next_key::<EntryMember>()? {
            None => Ok(ObjectListEntry(target)),
            Some(beside) => Err(A::Error::custom(ObjectListEntryRefusal::Crowded { member, beside })),
        }
    }
}

/// A member an `objectList` entry written as an object may hold.
#[derive(Debug, Clone, Copy, Deserialize, Display)]
#[serde(field_identifier, rename_all = "camelCase")]
enum EntryMember {
    /// The target of a Relationship Object.
    #[display("object")]
    Object,
    /// The target of a node reference.
    #[display("id")]
    Id,
}

/// Why an `objectList` entry written as an object names no target.
#[derive(Debug, Error)]
enum ObjectListEntryRefusal {
    /// The object holds no member.
    #[error("an objectList entry written as an object holds a single member, 'object' or 'id', and this one holds none")]
    Empty,
    /// The object holds a second member beside its target.
    #[error("an objectList entry written as an object holds a single member, and this one holds '{beside}' beside '{member}'")]
    Crowded {
        /// The member naming the target.
        member: EntryMember,
        /// The member written beside it.
        beside: EntryMember,
    },
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
mod tests {
    use crate::attribute::object_list::{NormalizedObjectList, RelationshipObjectList};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use serde_json::json;
    use std::str::FromStr;

    /// The refusal of `written` read as an `objectList`.
    fn refusal(written: &str) -> String {
        serde_json::from_str::<RelationshipObjectList>(written).expect_err(written).to_string()
    }

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

    #[test]
    fn a_target_that_is_no_uri_is_refused_with_the_uri_reason_in_every_spelling() {
        for written in [r#"["not a uri"]"#, r#"[{"object": "not a uri"}]"#, r#"[{"id": "not a uri"}]"#] {
            let refused = refusal(written);
            assert!(refused.starts_with("invalid URI: not a valid URN, URL or IRI"), "{written}: {refused}");
        }
    }

    #[test]
    fn an_entry_object_with_an_unknown_member_is_refused_naming_the_members_it_may_hold() {
        let refused = refusal(r#"[{"subject": "urn:ngsi-ld:Entity:1"}]"#);
        assert!(refused.starts_with("unknown field `subject`, expected `object` or `id`"), "{refused}");
    }

    /// Clause 4.5.22.2: a Relationship Object is "a JSON object containing a
    /// single Attribute with a key called `object`".
    #[test]
    fn an_entry_object_holding_no_member_or_a_second_one_is_refused() {
        let empty = refusal("[{}]");
        assert!(empty.contains("holds none"), "{empty}");
        let crowded = refusal(r#"[{"object": "urn:ngsi-ld:A:1", "id": "urn:ngsi-ld:A:2"}]"#);
        assert!(crowded.contains("holds 'id' beside 'object'"), "{crowded}");
        let unknown_beside = refusal(r#"[{"object": "urn:ngsi-ld:A:1", "extra": 1}]"#);
        assert!(unknown_beside.starts_with("unknown field `extra`"), "{unknown_beside}");
    }

    #[test]
    fn an_entry_of_another_kind_is_refused_naming_the_spellings() {
        for written in ["[42]", "[null]", "[[\"urn:ngsi-ld:A:1\"]]"] {
            let refused = refusal(written);
            assert!(
                refused.contains("expected a relationship target URI, an {\"object\": ...} object or an {\"id\": ...} node reference"),
                "{written}: {refused}"
            );
        }
    }
}
