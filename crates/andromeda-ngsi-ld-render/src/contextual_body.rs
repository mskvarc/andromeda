//! A typed resource written with its `@context` member first.
//!
//! Clause 6.3.6 has an `application/ld+json` body carry its `@context` as a
//! member of the resource object. A resource type serialises its own members;
//! [`ContextualBody`] writes the `@context` member ahead of them through the
//! serializer the writer hands it, so the body is produced in one pass with no
//! intermediate document, and without `#[serde(flatten)]`, which would buffer
//! the members into serde's `Content` tree first.

use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_vocabulary::keyword;
use serde::{
    Serialize,
    Serializer,
    ser::{Error, Impossible, SerializeMap, SerializeSeq, SerializeStruct},
};
use thiserror::Error;

/// An object opened on `serializer` with the `@context` member written first
/// when `context` is given.
///
/// Every writer that holds its own members opens its object here, so the
/// placement of `@context` is decided in this one place: first, as in
/// [`ContextualBody`] (clause 6.3.6).
///
/// # Errors
///
/// Returns the serializer's failure to open the object or to write the member.
pub(crate) fn open_object<S: Serializer>(serializer: S, context: Option<&JsonLiteral>, length: Option<usize>) -> Result<S::SerializeMap, S::Error> {
    match context {
        None => serializer.serialize_map(length),
        Some(context) => {
            let mut map = serializer.serialize_map(length.map(|members| members + 1))?;
            map.serialize_entry(keyword::CONTEXT, context)?;
            Ok(map)
        }
    }
}

/// A resource object with the `@context` it is written under.
///
/// Serialises as `body` does, with the `@context` member first when `context`
/// is present; without one it is `body` itself.
#[derive(Debug)]
pub struct ContextualBody<'a, T> {
    /// The context to embed, when the response carries it in the body.
    context: Option<&'a JsonLiteral>,
    /// The resource.
    body: &'a T,
}

impl<'a, T> ContextualBody<'a, T> {
    /// The resource `body`, embedding `context` when it is given.
    #[must_use]
    pub const fn new(context: Option<&'a JsonLiteral>, body: &'a T) -> ContextualBody<'a, T> {
        ContextualBody { context, body }
    }
}

impl<T: Serialize> Serialize for ContextualBody<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.context {
            None => self.body.serialize(serializer),
            Some(context) => self.body.serialize(ContextFirst { inner: serializer, context }),
        }
    }
}

/// An array of resource objects, each carrying its own `@context`.
///
/// An array of resources is not a JSON-LD document, so there is no enclosing
/// object the member could go on instead (clause 6.3.6).
#[derive(Debug)]
pub struct ContextualCollection<'a, T> {
    /// The context each item embeds, when the response carries it in the body.
    context: Option<&'a JsonLiteral>,
    /// The resources.
    items: &'a [T],
}

impl<'a, T> ContextualCollection<'a, T> {
    /// The resources `items`, each embedding `context` when it is given.
    #[must_use]
    pub const fn new(context: Option<&'a JsonLiteral>, items: &'a [T]) -> ContextualCollection<'a, T> {
        ContextualCollection { context, items }
    }
}

impl<T: Serialize> Serialize for ContextualCollection<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.items.len()))?;
        for item in self.items {
            sequence.serialize_element(&ContextualBody::new(self.context, item))?;
        }
        sequence.end()
    }
}

/// The refusal of a resource that serialises as something other than a JSON
/// object where a `@context` member has to be embedded.
#[derive(Debug, Error)]
#[error("a body that embeds an @context serialises as a JSON object")]
struct NotAnObject;

/// A serializer that opens the object it is asked for with the `@context`
/// member, and refuses anything that is no object.
struct ContextFirst<'a, S> {
    /// The serializer the object is written through.
    inner: S,
    /// The context written first.
    context: &'a JsonLiteral,
}

impl<S: Serializer> ContextFirst<'_, S> {
    /// The error of a resource that serialises as something other than an
    /// object.
    fn not_an_object() -> S::Error {
        S::Error::custom(NotAnObject)
    }

    /// The object opened and its `@context` member written.
    fn opened(self, length: Option<usize>) -> Result<S::SerializeMap, S::Error> {
        open_object(self.inner, Some(self.context), length)
    }
}

/// The scalar and sequence serializer methods of [`ContextFirst`], each
/// refusing: none of them writes an object.
macro_rules! refuse_non_objects {
    ($($method:ident($($argument:ty),*) -> $kind:ty;)*) => {
        $(
            fn $method(self, $(_: $argument),*) -> Result<$kind, S::Error> {
                Err(ContextFirst::<S>::not_an_object())
            }
        )*
    };
}

impl<S: Serializer> Serializer for ContextFirst<'_, S> {
    type Ok = S::Ok;
    type Error = S::Error;
    type SerializeSeq = Impossible<S::Ok, S::Error>;
    type SerializeTuple = Impossible<S::Ok, S::Error>;
    type SerializeTupleStruct = Impossible<S::Ok, S::Error>;
    type SerializeTupleVariant = Impossible<S::Ok, S::Error>;
    type SerializeMap = S::SerializeMap;
    type SerializeStruct = ContextFirstStruct<S::SerializeMap>;
    type SerializeStructVariant = Impossible<S::Ok, S::Error>;

    refuse_non_objects! {
        serialize_bool(bool) -> S::Ok;
        serialize_i8(i8) -> S::Ok;
        serialize_i16(i16) -> S::Ok;
        serialize_i32(i32) -> S::Ok;
        serialize_i64(i64) -> S::Ok;
        serialize_u8(u8) -> S::Ok;
        serialize_u16(u16) -> S::Ok;
        serialize_u32(u32) -> S::Ok;
        serialize_u64(u64) -> S::Ok;
        serialize_f32(f32) -> S::Ok;
        serialize_f64(f64) -> S::Ok;
        serialize_char(char) -> S::Ok;
        serialize_str(&str) -> S::Ok;
        serialize_bytes(&[u8]) -> S::Ok;
        serialize_unit() -> S::Ok;
        serialize_unit_struct(&'static str) -> S::Ok;
        serialize_unit_variant(&'static str, u32, &'static str) -> S::Ok;
        serialize_seq(Option<usize>) -> Self::SerializeSeq;
        serialize_tuple(usize) -> Self::SerializeTuple;
        serialize_tuple_struct(&'static str, usize) -> Self::SerializeTupleStruct;
        serialize_tuple_variant(&'static str, u32, &'static str, usize) -> Self::SerializeTupleVariant;
        serialize_struct_variant(&'static str, u32, &'static str, usize) -> Self::SerializeStructVariant;
    }

    fn serialize_none(self) -> Result<S::Ok, S::Error> {
        Err(ContextFirst::<S>::not_an_object())
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<S::Ok, S::Error> {
        value.serialize(self)
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(self, _name: &'static str, value: &T) -> Result<S::Ok, S::Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(self, _name: &'static str, _index: u32, _variant: &'static str, _value: &T) -> Result<S::Ok, S::Error> {
        Err(ContextFirst::<S>::not_an_object())
    }

    fn serialize_map(self, length: Option<usize>) -> Result<S::SerializeMap, S::Error> {
        self.opened(length)
    }

    fn serialize_struct(self, _name: &'static str, length: usize) -> Result<ContextFirstStruct<S::SerializeMap>, S::Error> {
        self.opened(Some(length)).map(|map| ContextFirstStruct { map })
    }
}

/// A struct written as a JSON object whose `@context` member is already out.
struct ContextFirstStruct<M> {
    /// The object the fields are written into.
    map: M,
}

impl<M: SerializeMap> SerializeStruct for ContextFirstStruct<M> {
    type Ok = M::Ok;
    type Error = M::Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Result<(), M::Error> {
        self.map.serialize_entry(key, value)
    }

    fn end(self) -> Result<M::Ok, M::Error> {
        self.map.end()
    }
}

#[cfg(test)]
mod tests {
    use crate::contextual_body::{ContextualBody, ContextualCollection};
    use andromeda_json_literal::JsonLiteral;
    use serde::Serialize;
    use std::collections::BTreeMap;

    /// A resource serialised as a struct.
    #[derive(Serialize)]
    struct Resource {
        id: &'static str,
        #[serde(rename = "type")]
        type_: &'static str,
    }

    /// A resource with a flattened group, which serde writes as a map.
    #[derive(Serialize)]
    struct Flattened {
        id: &'static str,
        #[serde(flatten)]
        rest: BTreeMap<&'static str, u8>,
    }

    fn context() -> JsonLiteral {
        JsonLiteral::admit(r#""https://example.org/ctx""#).expect("an admissible literal")
    }

    #[test]
    fn a_response_context_member_comes_first() {
        let context = context();
        let body = Resource { id: "urn:a", type_: "T" };
        let written = serde_json::to_string(&ContextualBody::new(Some(&context), &body)).expect("a written body");
        pretty_assertions::assert_eq!(written, r#"{"@context":"https://example.org/ctx","id":"urn:a","type":"T"}"#);
    }

    #[test]
    fn a_flattened_resource_carries_its_context_first_too() {
        let context = context();
        let body = Flattened {
            id: "urn:a",
            rest: BTreeMap::from([("n", 1)]),
        };
        let written = serde_json::to_string(&ContextualBody::new(Some(&context), &body)).expect("a written body");
        pretty_assertions::assert_eq!(written, r#"{"@context":"https://example.org/ctx","id":"urn:a","n":1}"#);
    }

    #[test]
    fn a_body_without_a_context_is_the_resource_itself() {
        let body = Resource { id: "urn:a", type_: "T" };
        let written = serde_json::to_string(&ContextualBody::new(None, &body)).expect("a written body");
        pretty_assertions::assert_eq!(written, r#"{"id":"urn:a","type":"T"}"#);
    }

    #[test]
    fn every_item_of_a_collection_carries_its_own_context() {
        let context = context();
        let items = [Resource { id: "urn:a", type_: "T" }, Resource { id: "urn:b", type_: "T" }];
        let written = serde_json::to_string(&ContextualCollection::new(Some(&context), &items)).expect("a written body");
        pretty_assertions::assert_eq!(
            written,
            r#"[{"@context":"https://example.org/ctx","id":"urn:a","type":"T"},{"@context":"https://example.org/ctx","id":"urn:b","type":"T"}]"#
        );
    }

    #[test]
    fn a_resource_that_is_no_object_is_refused() {
        let context = context();
        let refused = serde_json::to_string(&ContextualBody::new(Some(&context), &7_u8));
        assert!(refused.is_err(), "a scalar cannot carry a member");
    }
}
