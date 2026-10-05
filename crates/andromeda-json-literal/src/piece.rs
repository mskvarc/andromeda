//! One value inside an admitted literal, read in place from its text.
//!
//! A piece is a slice of the literal's text; looking inside it parses only
//! the level asked for, and every nested value stays a slice. Nothing here
//! builds a tree.

use crate::JsonLiteral;
use andromeda_ngsi_ld_number::JsonNumber;
use serde::de::{Deserialize, Deserializer, Error as DeserializeError, MapAccess, Visitor};
use serde_json::value::RawValue;
use std::{borrow::Cow, fmt, str::FromStr};

/// What kind of JSON value a piece is (IETF RFC 8259 section 3).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JsonKind {
    /// `null`.
    Null,
    /// `true` or `false`.
    Boolean,
    /// A number.
    Number,
    /// A string.
    String,
    /// An array.
    Array,
    /// An object.
    Object,
}

/// One value of an admitted literal: the literal itself, or a value nested in
/// it.
#[derive(Clone, Copy, Debug)]
pub struct JsonPiece<'a>(&'a RawValue);

impl<'a> JsonPiece<'a> {
    /// The piece the raw text of an admitted value is.
    pub(crate) const fn new(raw: &'a RawValue) -> JsonPiece<'a> {
        JsonPiece(raw)
    }

    /// The JSON text of the piece.
    #[must_use]
    pub fn text(self) -> &'a str {
        self.0.get()
    }

    /// What kind of value the piece is, told by its first character.
    #[must_use]
    pub fn kind(self) -> JsonKind {
        match self.text().trim_start().as_bytes().first() {
            Some(b'n') => JsonKind::Null,
            Some(b't' | b'f') => JsonKind::Boolean,
            Some(b'"') => JsonKind::String,
            Some(b'[') => JsonKind::Array,
            Some(b'{') => JsonKind::Object,
            Some(_) | None => JsonKind::Number,
        }
    }

    /// The string the piece is, unescaped.
    #[must_use]
    pub fn as_str(self) -> Option<Cow<'a, str>> {
        match self.kind() {
            JsonKind::String => serde_json::from_str::<Text<'a>>(self.text()).ok().map(|text| text.0),
            JsonKind::Null | JsonKind::Boolean | JsonKind::Number | JsonKind::Array | JsonKind::Object => None,
        }
    }

    /// The number the piece is.
    #[must_use]
    pub fn as_number(self) -> Option<JsonNumber> {
        match self.kind() {
            JsonKind::Number => JsonNumber::from_str(self.text().trim()).ok(),
            JsonKind::Null | JsonKind::Boolean | JsonKind::String | JsonKind::Array | JsonKind::Object => None,
        }
    }

    /// The boolean the piece is.
    #[must_use]
    pub fn as_bool(self) -> Option<bool> {
        match self.kind() {
            JsonKind::Boolean => Some(self.text().trim() == "true"),
            JsonKind::Null | JsonKind::Number | JsonKind::String | JsonKind::Array | JsonKind::Object => None,
        }
    }

    /// The elements of the array the piece is, in order.
    #[must_use]
    pub fn elements(self) -> Option<Vec<JsonPiece<'a>>> {
        match self.kind() {
            JsonKind::Array => self.array_elements().ok(),
            JsonKind::Null | JsonKind::Boolean | JsonKind::Number | JsonKind::String | JsonKind::Object => None,
        }
    }

    /// The elements of the array the piece is.
    ///
    /// # Errors
    ///
    /// Returns the reader's error when the piece is not an array.
    pub(crate) fn array_elements(self) -> Result<Vec<JsonPiece<'a>>, serde_json::Error> {
        serde_json::from_str::<Vec<&'a RawValue>>(self.text()).map(|elements| elements.into_iter().map(JsonPiece).collect())
    }

    /// The members of the object the piece is, in written order.
    #[must_use]
    pub fn members(self) -> Option<Vec<(Cow<'a, str>, JsonPiece<'a>)>> {
        match self.kind() {
            JsonKind::Object => self.object_members().ok(),
            JsonKind::Null | JsonKind::Boolean | JsonKind::Number | JsonKind::String | JsonKind::Array => None,
        }
    }

    /// The members of the object the piece is, in written order.
    ///
    /// # Errors
    ///
    /// Returns the reader's error when the piece is not an object.
    pub(crate) fn object_members(self) -> Result<Vec<(Cow<'a, str>, JsonPiece<'a>)>, serde_json::Error> {
        serde_json::from_str::<Members<'a>>(self.text()).map(|members| members.0)
    }

    /// The member of the object the piece is named `name`.
    #[must_use]
    pub fn member(self, name: &str) -> Option<JsonPiece<'a>> {
        self.members()?.into_iter().find_map(|(key, value)| (key == name).then_some(value))
    }

    /// The value the member path `names` leads to from this piece, each name
    /// stepping into a member of an object.
    #[must_use]
    pub fn member_path<'n, I: IntoIterator<Item = &'n str>>(self, names: I) -> Option<JsonPiece<'a>> {
        names.into_iter().try_fold(self, JsonPiece::member)
    }

    /// How many containers deep the piece nests: none for a scalar, and one
    /// more than its deepest element or member for an array or an object.
    #[must_use]
    pub fn levels(self) -> usize {
        let deepest = |levels: Option<usize>| levels.unwrap_or(0).saturating_add(1);
        match self.kind() {
            JsonKind::Array => deepest(self.elements().into_iter().flatten().map(JsonPiece::levels).max()),
            JsonKind::Object => deepest(self.members().into_iter().flatten().map(|(_, value)| value.levels()).max()),
            JsonKind::Null | JsonKind::Boolean | JsonKind::Number | JsonKind::String => 0,
        }
    }

    /// The piece as a literal of its own.
    ///
    /// The piece was admitted with the literal it belongs to, so its text is
    /// taken as it stands.
    #[must_use]
    pub fn to_literal(self) -> JsonLiteral {
        JsonLiteral(self.0.to_owned())
    }
}

/// A string read without copying it when it carries no escape.
struct Text<'a>(Cow<'a, str>);

impl<'de> Deserialize<'de> for Text<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Text<'de>, D::Error> {
        deserializer.deserialize_str(TextVisitor)
    }
}

/// Reads one string, borrowed from the text when it can be.
struct TextVisitor;

impl<'de> Visitor<'de> for TextVisitor {
    type Value = Text<'de>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON string")
    }

    fn visit_borrowed_str<E: DeserializeError>(self, text: &'de str) -> Result<Text<'de>, E> {
        Ok(Text(Cow::Borrowed(text)))
    }

    fn visit_str<E: DeserializeError>(self, text: &str) -> Result<Text<'de>, E> {
        Ok(Text(Cow::Owned(text.to_owned())))
    }

    fn visit_string<E: DeserializeError>(self, text: String) -> Result<Text<'de>, E> {
        Ok(Text(Cow::Owned(text)))
    }
}

/// The members of one object, each value left as text.
struct Members<'a>(Vec<(Cow<'a, str>, JsonPiece<'a>)>);

impl<'de> Deserialize<'de> for Members<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Members<'de>, D::Error> {
        deserializer.deserialize_map(MembersVisitor)
    }
}

/// Reads the members of one object in written order.
struct MembersVisitor;

impl<'de> Visitor<'de> for MembersVisitor {
    type Value = Members<'de>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Members<'de>, A::Error> {
        let mut members = Vec::with_capacity(map.size_hint().unwrap_or(0));
        while let Some(Text(name)) = map.next_key::<Text<'de>>()? {
            let value: &'de RawValue = map.next_value()?;
            members.push((name, JsonPiece(value)));
        }
        Ok(Members(members))
    }
}
