//! What a request body holds at its top level, read without building the
//! body.
//!
//! The clause 6.3.5 rules on a body's `@context` -- an `application/json`
//! body must not carry one, an `application/ld+json` body must -- and the
//! clause 5.5.4 rules on its shape are decided on the members at the top of
//! the body alone. They are read from the text straight away, every other
//! member skipped unread, so a body carrying a geometry of a million positions
//! is never built into a JSON tree to find out whether it names a context. No
//! JSON tree is built at all: the reader is the minimal typed scan the
//! extractors need (an empty array, the presence, shape and value of the
//! `@context`, and a string `id`).

use andromeda_json_literal::{JsonLiteral, piece::JsonKind};
use andromeda_ngsi_ld_number::json_value::value_census::MemberName;
use serde::{
    Deserialize,
    de::{DeserializeSeed, Deserializer, Error as DeserializeError, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use serde_json::Deserializer as JsonReader;
use std::fmt;
use thiserror::Error;

/// The member a JSON-LD document names its context in.
const CONTEXT_MEMBER: &str = "@context";

/// The member an NGSI-LD entity names its identifier in.
const ID_MEMBER: &str = "id";

/// Why the `@context` of an `application/ld+json` body cannot be read.
#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum BodyContextError {
    /// Clause 6.3.5: an `application/ld+json` body names its context in its
    /// `@context` member, and this one has none.
    #[error("application/ld+json body must contain @context")]
    Missing,
    /// JSON-LD applies a top-level node's `@context` to that node alone, so the
    /// members of an array body read as one request must name one and the same
    /// context; a batch operation, whose items are separate entities, is read
    /// item by item instead.
    #[error("every member of an application/ld+json array body must carry the same @context")]
    Divergent,
}

/// The top-level members of a JSON object the request rules ask about.
#[derive(Debug, Default)]
pub(crate) struct ObjectEnvelope {
    /// The `@context` member, as written.
    context: Option<JsonLiteral>,
    /// The `id` member, when it is a string.
    id: Option<String>,
}

impl ObjectEnvelope {
    /// The `@context` member, as written.
    pub(crate) const fn context(&self) -> Option<&JsonLiteral> {
        self.context.as_ref()
    }

    /// Take the `@context` member, as written.
    pub(crate) fn into_context(self) -> Option<JsonLiteral> {
        self.context
    }

    /// The `id` member, when it is a string.
    pub(crate) fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }
}

/// What a request body is at its top level.
#[derive(Debug)]
pub(crate) enum BodyEnvelope {
    /// An object, and the members of it the rules ask about.
    Object(ObjectEnvelope),
    /// An array, and for each item that is an object, the members of it the
    /// rules ask about.
    Array(Vec<Option<ObjectEnvelope>>),
    /// `null`.
    Null,
    /// A string, a number or a boolean.
    Scalar,
}

impl BodyEnvelope {
    /// Read the top level of the JSON text `text`.
    ///
    /// # Errors
    ///
    /// Returns the streaming reader's failure for a text that is not one JSON
    /// value, trailing content included.
    pub(crate) fn read(text: &[u8]) -> Result<BodyEnvelope, serde_json::Error> {
        let mut reader = JsonReader::from_slice(text);
        let envelope = EnvelopeSeed.deserialize(&mut reader)?;
        reader.end()?;
        Ok(envelope)
    }

    /// Whether the body is an array without items.
    pub(crate) const fn is_empty_array(&self) -> bool {
        matches!(self, BodyEnvelope::Array(items) if items.is_empty())
    }

    /// Whether the body, or one of its items, carries a top-level `@context`.
    pub(crate) fn has_context(&self) -> bool {
        self.contexts().next().is_some()
    }

    /// Whether every top-level `@context` the body carries has one of the
    /// shapes JSON-LD 1.1 section 3.1 gives it: an IRI string, a context
    /// definition object, or an array of those (and `null`, which resets).
    /// Clause 5.5.4: any other shape is data that is not syntactically correct
    /// according to the API data type definitions.
    pub(crate) fn contexts_are_well_formed(&self) -> bool {
        let is_entry = |kind: JsonKind| matches!(kind, JsonKind::Null | JsonKind::String | JsonKind::Object);
        self.contexts().all(|context| match context.piece().kind() {
            JsonKind::Null | JsonKind::String | JsonKind::Object => true,
            JsonKind::Array => context.piece().elements().unwrap_or_default().into_iter().all(|item| is_entry(item.kind())),
            JsonKind::Boolean | JsonKind::Number => false,
        })
    }

    /// The `@context` an `application/ld+json` body names, as written.
    ///
    /// An object names its own; an array names the one every member carries.
    ///
    /// # Errors
    ///
    /// Returns [`BodyContextError::Missing`] for a body naming none and
    /// [`BodyContextError::Divergent`] for an array whose members name
    /// different ones.
    pub(crate) fn into_body_context(self) -> Result<JsonLiteral, BodyContextError> {
        match self {
            BodyEnvelope::Object(object) => object.into_context().ok_or(BodyContextError::Missing),
            BodyEnvelope::Array(items) => {
                let mut contexts = items.into_iter().map(|item| item.and_then(ObjectEnvelope::into_context));
                let first = contexts.next().flatten().ok_or(BodyContextError::Missing)?;
                if contexts.all(|context| context.as_ref() == Some(&first)) {
                    Ok(first)
                } else {
                    Err(BodyContextError::Divergent)
                }
            }
            BodyEnvelope::Null | BodyEnvelope::Scalar => Err(BodyContextError::Missing),
        }
    }

    /// Every top-level `@context` the body carries.
    fn contexts(&self) -> impl Iterator<Item = &JsonLiteral> {
        let (own, items) = match self {
            BodyEnvelope::Object(object) => (object.context(), &[][..]),
            BodyEnvelope::Array(items) => (None, items.as_slice()),
            BodyEnvelope::Null | BodyEnvelope::Scalar => (None, &[][..]),
        };
        own.into_iter().chain(items.iter().flatten().filter_map(ObjectEnvelope::context))
    }
}

/// Reads the top level of a body.
#[derive(Clone, Copy, Debug)]
struct EnvelopeSeed;

impl<'de> DeserializeSeed<'de> for EnvelopeSeed {
    type Value = BodyEnvelope;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<BodyEnvelope, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for EnvelopeSeed {
    type Value = BodyEnvelope;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E: DeserializeError>(self, _value: bool) -> Result<BodyEnvelope, E> {
        Ok(BodyEnvelope::Scalar)
    }

    fn visit_i64<E: DeserializeError>(self, _value: i64) -> Result<BodyEnvelope, E> {
        Ok(BodyEnvelope::Scalar)
    }

    fn visit_u64<E: DeserializeError>(self, _value: u64) -> Result<BodyEnvelope, E> {
        Ok(BodyEnvelope::Scalar)
    }

    fn visit_f64<E: DeserializeError>(self, _value: f64) -> Result<BodyEnvelope, E> {
        Ok(BodyEnvelope::Scalar)
    }

    fn visit_str<E: DeserializeError>(self, _value: &str) -> Result<BodyEnvelope, E> {
        Ok(BodyEnvelope::Scalar)
    }

    fn visit_unit<E: DeserializeError>(self) -> Result<BodyEnvelope, E> {
        Ok(BodyEnvelope::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<BodyEnvelope, A::Error> {
        let mut read = Vec::new();
        while let Some(item) = items.next_element_seed(ItemSeed)? {
            read.push(item);
        }
        Ok(BodyEnvelope::Array(read))
    }

    fn visit_map<A: MapAccess<'de>>(self, members: A) -> Result<BodyEnvelope, A::Error> {
        Ok(read_object(members)?.map_or(BodyEnvelope::Scalar, BodyEnvelope::Object))
    }
}

/// Reads one item of an array body: the members the rules ask about when it
/// is an object, nothing otherwise.
#[derive(Clone, Copy, Debug)]
struct ItemSeed;

impl<'de> DeserializeSeed<'de> for ItemSeed {
    type Value = Option<ObjectEnvelope>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Option<ObjectEnvelope>, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for ItemSeed {
    type Value = Option<ObjectEnvelope>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E: DeserializeError>(self, _value: bool) -> Result<Option<ObjectEnvelope>, E> {
        Ok(None)
    }

    fn visit_i64<E: DeserializeError>(self, _value: i64) -> Result<Option<ObjectEnvelope>, E> {
        Ok(None)
    }

    fn visit_u64<E: DeserializeError>(self, _value: u64) -> Result<Option<ObjectEnvelope>, E> {
        Ok(None)
    }

    fn visit_f64<E: DeserializeError>(self, _value: f64) -> Result<Option<ObjectEnvelope>, E> {
        Ok(None)
    }

    fn visit_str<E: DeserializeError>(self, _value: &str) -> Result<Option<ObjectEnvelope>, E> {
        Ok(None)
    }

    fn visit_unit<E: DeserializeError>(self) -> Result<Option<ObjectEnvelope>, E> {
        Ok(None)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<Option<ObjectEnvelope>, A::Error> {
        while items.next_element::<IgnoredAny>()?.is_some() {}
        Ok(None)
    }

    fn visit_map<A: MapAccess<'de>>(self, members: A) -> Result<Option<ObjectEnvelope>, A::Error> {
        read_object(members)
    }
}

/// The `id` member of an object, when it is a string; any other value is read
/// to its end and is no identifier.
#[derive(Debug)]
struct IdMember(Option<String>);

impl<'de> Deserialize<'de> for IdMember {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<IdMember, D::Error> {
        deserializer.deserialize_any(IdMemberVisitor)
    }
}

/// Reads the `id` member of an object.
#[derive(Clone, Copy, Debug)]
struct IdMemberVisitor;

impl<'de> Visitor<'de> for IdMemberVisitor {
    type Value = IdMember;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E: DeserializeError>(self, _value: bool) -> Result<IdMember, E> {
        Ok(IdMember(None))
    }

    fn visit_i64<E: DeserializeError>(self, _value: i64) -> Result<IdMember, E> {
        Ok(IdMember(None))
    }

    fn visit_u64<E: DeserializeError>(self, _value: u64) -> Result<IdMember, E> {
        Ok(IdMember(None))
    }

    fn visit_f64<E: DeserializeError>(self, _value: f64) -> Result<IdMember, E> {
        Ok(IdMember(None))
    }

    fn visit_str<E: DeserializeError>(self, value: &str) -> Result<IdMember, E> {
        Ok(IdMember(Some(value.to_owned())))
    }

    fn visit_string<E: DeserializeError>(self, value: String) -> Result<IdMember, E> {
        Ok(IdMember(Some(value)))
    }

    fn visit_unit<E: DeserializeError>(self) -> Result<IdMember, E> {
        Ok(IdMember(None))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<IdMember, A::Error> {
        while items.next_element::<IgnoredAny>()?.is_some() {}
        Ok(IdMember(None))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut members: A) -> Result<IdMember, A::Error> {
        while members.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(IdMember(None))
    }
}

/// Read the members of an object the rules ask about, every other member
/// skipped unread; a member written twice counts as written last, as a parsed
/// tree keeps it.
///
/// An object whose first member carries the reader's private number name is
/// the reader's spelling of a number, and no object at all.
fn read_object<'de, A: MapAccess<'de>>(mut members: A) -> Result<Option<ObjectEnvelope>, A::Error> {
    let mut envelope = ObjectEnvelope::default();
    let mut first = true;
    while let Some(name) = members.next_key::<String>()? {
        if first && MemberName::of(&name) == MemberName::NumberToken {
            members.next_value::<IgnoredAny>()?;
            return Ok(None);
        }
        first = false;
        match name.as_str() {
            CONTEXT_MEMBER => envelope.context = Some(members.next_value()?),
            ID_MEMBER => envelope.id = members.next_value::<IdMember>()?.0,
            _other => {
                members.next_value::<IgnoredAny>()?;
            }
        }
    }
    Ok(Some(envelope))
}

#[cfg(test)]
mod tests {
    use crate::body_envelope::{BodyContextError, BodyEnvelope};
    use andromeda_json_literal::JsonLiteral;
    use serde_json::json;

    fn literal(value: &serde_json::Value) -> JsonLiteral {
        JsonLiteral::admit(&value.to_string()).expect("an admissible context")
    }

    fn envelope(text: &str) -> BodyEnvelope {
        BodyEnvelope::read(text.as_bytes()).expect("valid JSON")
    }

    #[test]
    fn an_object_names_its_own_context_and_id() {
        let read = envelope(r#"{"id":"urn:ngsi-ld:T:1","n":{"value":[1,2,3]},"@context":["https://example.org/a"]}"#);
        assert!(read.has_context());
        let BodyEnvelope::Object(object) = &read else {
            panic!("an object");
        };
        pretty_assertions::assert_eq!(object.id(), Some("urn:ngsi-ld:T:1"));
        pretty_assertions::assert_eq!(read.into_body_context(), Ok(literal(&json!(["https://example.org/a"]))));
    }

    #[test]
    fn an_id_that_is_no_string_is_read_to_its_end_and_is_no_identifier() {
        for text in [
            r#"{"id":5,"@context":"x"}"#,
            r#"{"id":{"a":[1,{"b":null}]},"@context":"x"}"#,
            r#"{"id":[1,2],"@context":"x"}"#,
            r#"{"id":null,"@context":"x"}"#,
        ] {
            let read = envelope(text);
            let BodyEnvelope::Object(object) = &read else {
                panic!("an object: {text}");
            };
            pretty_assertions::assert_eq!(object.id(), None, "{text}");
            assert!(read.has_context(), "{text}");
        }
    }

    #[test]
    fn an_array_names_the_context_every_member_carries() {
        let shared = envelope(r#"[{"@context":"https://example.org/a"},{"@context":"https://example.org/a"}]"#);
        pretty_assertions::assert_eq!(shared.into_body_context(), Ok(literal(&json!("https://example.org/a"))));
        let divergent = envelope(r#"[{"@context":"https://example.org/a"},{"@context":"https://example.org/b"}]"#);
        pretty_assertions::assert_eq!(divergent.into_body_context(), Err(BodyContextError::Divergent));
        let partial = envelope(r#"[{"@context":"https://example.org/a"},7]"#);
        pretty_assertions::assert_eq!(partial.into_body_context(), Err(BodyContextError::Divergent));
        let missing = envelope(r#"[7,{"@context":"https://example.org/a"}]"#);
        pretty_assertions::assert_eq!(missing.into_body_context(), Err(BodyContextError::Missing));
    }

    #[test]
    fn a_context_of_the_wrong_shape_is_not_well_formed() {
        for text in [
            r#"{"@context":42}"#,
            r#"{"@context":true}"#,
            r#"{"@context":[1,"https://example.org/ctx"]}"#,
            r#"[{"@context":7}]"#,
        ] {
            assert!(!envelope(text).contexts_are_well_formed(), "{text}");
        }
        for text in [r#"{"@context":null}"#, r#"{"@context":[null,{},"x"]}"#, "{}", "[1]"] {
            assert!(envelope(text).contexts_are_well_formed(), "{text}");
        }
    }

    #[test]
    fn the_last_context_written_is_the_one_read() {
        pretty_assertions::assert_eq!(envelope(r#"{"@context":"a","@context":"b"}"#).into_body_context(), Ok(literal(&json!("b"))));
    }

    #[test]
    fn the_shape_of_the_body_is_told_apart() {
        assert!(envelope("[]").is_empty_array());
        assert!(!envelope("[1]").is_empty_array());
        assert!(matches!(envelope("null"), BodyEnvelope::Null));
        assert!(matches!(envelope("\"text\""), BodyEnvelope::Scalar));
        assert!(matches!(envelope(r#"{"$serde_json::private::Number":"5"}"#), BodyEnvelope::Scalar));
    }
}
