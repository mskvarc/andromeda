//! The compacted document the term walk writes.
//!
//! A compacted document is JSON, with one addition: a `GeoProperty` geometry
//! the compact model reads as the typed geometry it is can stay that geometry,
//! borrowed from the entity state, rather than being written out position by
//! position only for the model to read the positions back. Such a tree is read
//! straight into the compact model ([`crate::compacted_node`]); written out as
//! JSON, a typed geometry is written as its type and positions.
//!
//! Objects keep their members in the order of their names, which is the order
//! the JSON object of the document holds them in.

use andromeda_ngsi_ld_geometry::form::GeometryForm;
use jsonld_syntax::{NumberBuf, Value as JsonValue};
use serde::{Serialize, Serializer};
use std::collections::{BTreeMap, btree_map::Iter as MemberIter};

/// One value of a compacted document.
#[derive(Debug, Clone)]
pub(crate) enum Compacted<'a> {
    /// `null`.
    Null,
    /// `true` or `false`.
    Boolean(bool),
    /// A number, in its compacted spelling.
    Number(NumberBuf),
    /// A string.
    String(Box<str>),
    /// An array.
    Array(Vec<Compacted<'a>>),
    /// An object.
    Object(CompactedObject<'a>),
    /// A `GeoJSON` geometry object the compact model reads as typed, held as
    /// the geometry it is.
    Geometry(&'a GeometryForm),
}

/// The members of one compacted object, by name.
#[derive(Debug, Clone, Default)]
pub(crate) struct CompactedObject<'a>(BTreeMap<String, Compacted<'a>>);

impl<'a> CompactedObject<'a> {
    /// An object without members.
    pub(crate) const fn new() -> CompactedObject<'a> {
        CompactedObject(BTreeMap::new())
    }

    /// Set the member `key` to `value`, replacing whatever it held.
    pub(crate) fn insert(&mut self, key: &str, value: Compacted<'a>) {
        self.0.insert(key.to_owned(), value);
    }

    /// The member `key`.
    pub(crate) fn get(&self, key: &str) -> Option<&Compacted<'a>> {
        self.0.get(key)
    }

    /// The member `key`, for mutation.
    pub(crate) fn get_mut(&mut self, key: &str) -> Option<&mut Compacted<'a>> {
        self.0.get_mut(key)
    }

    /// How many members the object has.
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// The members, in the order of their names.
    pub(crate) fn iter(&self) -> MemberIter<'_, String, Compacted<'a>> {
        self.0.iter()
    }
}

impl Compacted<'_> {
    /// Whether the value is `null`.
    pub(crate) const fn is_null(&self) -> bool {
        matches!(self, Compacted::Null)
    }

    /// Whether the value is an array.
    pub(crate) const fn is_array(&self) -> bool {
        matches!(self, Compacted::Array(_))
    }

    /// The value as a string slice, when it is a string.
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Compacted::String(text) => Some(text),
            Compacted::Null | Compacted::Boolean(_) | Compacted::Number(_) | Compacted::Array(_) | Compacted::Object(_) | Compacted::Geometry(_) => None,
        }
    }
}

// Hand-written: a compacted value is written as the JSON it stands for, a
// number in its compacted spelling and a typed geometry as its type and
// positions, which no derive over the variants writes.
impl Serialize for Compacted<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Compacted::Null => serializer.serialize_unit(),
            Compacted::Boolean(flag) => serializer.serialize_bool(*flag),
            Compacted::Number(number) => serde_json::Number::from(number.clone()).serialize(serializer), // clone: the written number is the document's own
            Compacted::String(text) => serializer.serialize_str(text),
            Compacted::Array(items) => serializer.collect_seq(items),
            Compacted::Object(members) => serializer.collect_map(members.iter()),
            Compacted::Geometry(form) => form.serialize(serializer),
        }
    }
}

impl<'a> Compacted<'a> {
    /// The value as an object, for mutation, when it is one.
    pub(crate) const fn as_object_mut(&mut self) -> Option<&mut CompactedObject<'a>> {
        match self {
            Compacted::Object(members) => Some(members),
            Compacted::Null | Compacted::Boolean(_) | Compacted::Number(_) | Compacted::String(_) | Compacted::Array(_) | Compacted::Geometry(_) => None,
        }
    }
}

// A JSON value the JSON-LD library compacted is a compacted value as it
// stands; the conversion moves it member by member, which no derive writes.
impl<'a> From<JsonValue> for Compacted<'a> {
    fn from(value: JsonValue) -> Compacted<'a> {
        match value {
            JsonValue::Null => Compacted::Null,
            JsonValue::Boolean(flag) => Compacted::Boolean(flag),
            JsonValue::Number(number) => Compacted::Number(number),
            JsonValue::String(text) => Compacted::String(Box::from(text.as_str())),
            JsonValue::Array(items) => Compacted::Array(items.into_iter().map(Compacted::from).collect()),
            JsonValue::Object(members) => {
                let mut object = CompactedObject::new();
                for entry in members {
                    object.insert(entry.key.as_str(), Compacted::from(entry.value));
                }
                Compacted::Object(object)
            }
        }
    }
}
