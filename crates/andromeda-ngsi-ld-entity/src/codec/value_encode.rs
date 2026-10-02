//! Writing expanded values: value objects, JSON literals, nodes and lists.
//!
//! Each value is written in the expanded shape JSON-LD expansion gives it, so
//! what is written is what a standard processor reads. Numbers are written in
//! the exact canonical spelling [`JsonNumber`](andromeda_ngsi_ld_number::JsonNumber)
//! keeps.

use crate::{
    codec::element::JSON_DATATYPE,
    expanded_value::{ExpandedValue, ValueNode},
    json_literal::JsonLiteral,
    literal::{Literal, LiteralValue},
    value_mode::{Fragment, FragmentMember, FragmentText, Stored, ValueMode},
};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
use andromeda_ngsi_ld_vocabulary::NULL_URI;
use serde::{Serialize, Serializer, ser::SerializeMap};

/// How the members and texts of one value mode are written.
pub(crate) trait ModeEncoding: ValueMode {
    /// Write one member of a structured value.
    fn serialize_member<S: Serializer>(member: &Self::Member, serializer: S) -> Result<S::Ok, S::Error>;

    /// The text of one language entry; the NGSI-LD Null is written as itself.
    fn text(text: &Self::Text) -> &str;
}

impl ModeEncoding for Stored {
    fn serialize_member<S: Serializer>(member: &Vec<ExpandedValue<Stored>>, serializer: S) -> Result<S::Ok, S::Error> {
        ValuesOut(member).serialize(serializer)
    }

    fn text(text: &Self::Text) -> &str {
        text
    }
}

impl ModeEncoding for Fragment {
    fn serialize_member<S: Serializer>(member: &FragmentMember, serializer: S) -> Result<S::Ok, S::Error> {
        match member {
            FragmentMember::Values(values) => ValuesOut(values).serialize(serializer),
            FragmentMember::Remove => [NullValueOut].serialize(serializer),
        }
    }

    fn text(text: &FragmentText) -> &str {
        match text {
            FragmentText::Text(text) => text,
            FragmentText::Remove => NULL_URI.as_str(),
        }
    }
}

/// An array of expanded values.
pub(crate) struct ValuesOut<'a, M: ValueMode>(pub(crate) &'a [ExpandedValue<M>]);

impl<M: ModeEncoding> Serialize for ValuesOut<'_, M> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(ValueOut))
    }
}

/// One expanded value.
pub(crate) struct ValueOut<'a, M: ValueMode>(pub(crate) &'a ExpandedValue<M>);

impl<M: ModeEncoding> Serialize for ValueOut<'_, M> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            ExpandedValue::Literal(literal) => LiteralOut(literal).serialize(serializer),
            ExpandedValue::Json(json) => JsonLiteralOut(json).serialize(serializer),
            ExpandedValue::Node(node) => NodeOut(node).serialize(serializer),
            ExpandedValue::List(items) => ListOut(ValuesOut(items)).serialize(serializer),
        }
    }
}

/// A value object.
pub(crate) struct LiteralOut<'a>(pub(crate) &'a Literal);

impl Serialize for LiteralOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let literal = self.0;
        let mut map = serializer.serialize_map(None)?;
        if let Some(datatype) = literal.datatype() {
            map.serialize_entry("@type", datatype.as_str())?;
        }
        match literal.value() {
            LiteralValue::Number(number) => map.serialize_entry("@value", number)?,
            LiteralValue::String(text) => map.serialize_entry("@value", text.as_str())?,
            LiteralValue::Boolean(flag) => map.serialize_entry("@value", flag)?,
        }
        if let Some(language) = literal.language() {
            map.serialize_entry("@language", language.as_str())?;
        }
        if let Some(direction) = literal.direction() {
            map.serialize_entry("@direction", <&'static str>::from(direction))?;
        }
        map.end()
    }
}

/// A JSON literal.
pub(crate) struct JsonLiteralOut<'a>(pub(crate) &'a JsonLiteral);

impl Serialize for JsonLiteralOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("@type", JSON_DATATYPE)?;
        map.serialize_entry("@value", self.0.get())?;
        map.end()
    }
}

/// A node object or node reference.
pub(crate) struct NodeOut<'a, M: ValueMode>(pub(crate) &'a ValueNode<M>);

impl<M: ModeEncoding> Serialize for NodeOut<'_, M> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let node = self.0;
        let mut map = serializer.serialize_map(None)?;
        if let Some(id) = node.id() {
            map.serialize_entry("@id", id.as_str())?;
        }
        if !node.types().is_empty() {
            map.serialize_entry("@type", &TypesOut(node.types()))?;
        }
        for (name, member) in node.members() {
            map.serialize_entry(name.as_str(), &MemberOut::<M>(member))?;
        }
        map.end()
    }
}

/// The `@type` array of a node.
struct TypesOut<'a>(&'a [NgsiLdReference]);

impl Serialize for TypesOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(NgsiLdReference::as_str))
    }
}

/// One member of a structured value.
struct MemberOut<'a, M: ValueMode>(&'a M::Member);

impl<M: ModeEncoding> Serialize for MemberOut<'_, M> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        M::serialize_member(self.0, serializer)
    }
}

/// A list object wrapping already written items.
pub(crate) struct ListOut<T>(pub(crate) T);

impl<T: Serialize> Serialize for ListOut<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("@list", &self.0)?;
        map.end()
    }
}

/// `{"@value": "urn:ngsi-ld:null"}`: the NGSI-LD Null as a value object.
pub(crate) struct NullValueOut;

impl Serialize for NullValueOut {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("@value", NULL_URI.as_str())?;
        map.end()
    }
}

/// `{"@id": "<iri>"}`: a node reference.
pub(crate) struct IdOut<'a>(pub(crate) &'a str);

impl Serialize for IdOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("@id", self.0)?;
        map.end()
    }
}

/// `{"@value": "<text>"}`: a plain string value object.
pub(crate) struct TextOut<'a>(pub(crate) &'a str);

impl Serialize for TextOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("@value", self.0)?;
        map.end()
    }
}

/// `{"@type": "<datatype>", "@value": "<text>"}`: a typed string value object.
pub(crate) struct TypedTextOut<'a> {
    /// The datatype IRI.
    pub(crate) datatype: &'a str,
    /// The text.
    pub(crate) text: &'a str,
}

impl Serialize for TypedTextOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("@type", self.datatype)?;
        map.serialize_entry("@value", self.text)?;
        map.end()
    }
}
