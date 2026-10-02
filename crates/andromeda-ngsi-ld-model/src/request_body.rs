//! Reading a request body straight from the JSON text it arrived as.
//!
//! An NGSI-LD request body is decoded from its text, never from a JSON tree
//! parsed first: a tree costs an allocation per value, and an Entity carrying
//! a geometry of a million positions is millions of values. Its top-level
//! `@context` member is not part of the resource: clause 6.3.5 has an
//! `application/ld+json` body name its context there, and the transport reads
//! it before the body is decoded, so the decoding skips it.

use crate::{
    entity::{Entity, EntityPatch},
    entity_decoding::{EntityPatchVisitor, EntityVisitor, FromRequestText},
    query::body::Query,
    snapshot_clone::SnapshotCloneInput,
    snapshot_input::SnapshotInput,
    snapshot_patch::SnapshotPatch,
};
use serde::{
    Deserialize,
    Deserializer,
    de::{
        DeserializeOwned,
        DeserializeSeed,
        Error as DeserializeError,
        IgnoredAny,
        MapAccess,
        Visitor,
        value::{BorrowedStrDeserializer, MapAccessDeserializer, StringDeserializer},
    },
};
use serde_json::Deserializer as JsonReader;
use std::{fmt, marker::PhantomData};

/// The member an `application/ld+json` body names its context in.
const CONTEXT_MEMBER: &str = "@context";

/// A resource an NGSI-LD request body is decoded into.
pub trait RequestBody: DeserializeOwned {
    /// Decode the resource from the JSON text of a request body, skipping its
    /// top-level `@context` member.
    ///
    /// The result is the one the type's own decoding gives for the body with
    /// that member removed, read as `serde_json`'s streaming reader reads it.
    ///
    /// # Errors
    ///
    /// Returns the reader's failure, positioned in the text.
    fn from_request_text(text: &[u8]) -> Result<Self, serde_json::Error> {
        read_context_free(text, DeserializedFromMembers::<Self>(PhantomData))
    }
}

impl RequestBody for Query {}

impl RequestBody for SnapshotInput {}

impl RequestBody for SnapshotPatch {}

impl RequestBody for SnapshotCloneInput {}

impl RequestBody for Entity {
    /// Decode an Entity from the JSON text of a request body.
    ///
    /// Each attribute member is decoded from the slice of text it is, a
    /// `GeoProperty`'s geometry without any JSON tree, and every number the
    /// entity holds as JSON is given the canonical spelling a request admitted
    /// as a parsed tree gives it.
    fn from_request_text(text: &[u8]) -> Result<Entity, serde_json::Error> {
        let mut entity = read_context_free(text, EntityVisitor::<FromRequestText>::new())?;
        entity.canonicalize_numbers().map_err(serde_json::Error::custom)?;
        Ok(entity)
    }
}

impl RequestBody for EntityPatch {
    /// Decode a partial Entity from the JSON text of a request body, as
    /// [`Entity`] is decoded.
    fn from_request_text(text: &[u8]) -> Result<EntityPatch, serde_json::Error> {
        let mut patch = read_context_free(text, EntityPatchVisitor::<FromRequestText>::new())?;
        patch.canonicalize_numbers().map_err(serde_json::Error::custom)?;
        Ok(patch)
    }
}

/// Read the object `text` holds with `visitor`, its `@context` member
/// skipped.
///
/// # Errors
///
/// Returns the reader's failure, trailing content included.
fn read_context_free<'de, V: Visitor<'de>>(text: &'de [u8], visitor: V) -> Result<V::Value, serde_json::Error> {
    let mut reader = JsonReader::from_slice(text);
    let read = (&mut reader).deserialize_map(ContextFree(visitor))?;
    reader.end()?;
    Ok(read)
}

/// Hands the members of an object, its `@context` skipped, to the visitor it
/// wraps.
#[derive(Debug)]
struct ContextFree<V>(V);

impl<'de, V: Visitor<'de>> Visitor<'de> for ContextFree<V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.expecting(formatter)
    }

    fn visit_map<A: MapAccess<'de>>(self, members: A) -> Result<V::Value, A::Error> {
        self.0.visit_map(ContextFreeMembers { members })
    }
}

/// The members of an object without its `@context`.
#[derive(Debug)]
struct ContextFreeMembers<A> {
    /// The members as written.
    members: A,
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for ContextFreeMembers<A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, A::Error> {
        loop {
            match self.members.next_key::<MemberName<'de>>()? {
                None => return Ok(None),
                Some(MemberName::Borrowed(CONTEXT_MEMBER)) => {
                    self.members.next_value::<IgnoredAny>()?;
                }
                Some(MemberName::Owned(name)) if name == CONTEXT_MEMBER => {
                    self.members.next_value::<IgnoredAny>()?;
                }
                Some(MemberName::Borrowed(name)) => return seed.deserialize(BorrowedStrDeserializer::new(name)).map(Some),
                Some(MemberName::Owned(name)) => return seed.deserialize(StringDeserializer::new(name)).map(Some),
            }
        }
    }

    fn next_value_seed<S: DeserializeSeed<'de>>(&mut self, seed: S) -> Result<S::Value, A::Error> {
        self.members.next_value_seed(seed)
    }
}

/// One member name as the reader handed it out: borrowed from the text where
/// it could be, owned where an escape had to be decoded.
#[derive(Debug)]
enum MemberName<'de> {
    /// A name borrowed from the text.
    Borrowed(&'de str),
    /// A name decoded into text of its own.
    Owned(String),
}

// Hand-written: serde's borrowed-or-owned string decoding (`Cow`) always
// takes ownership, and the borrowed name is what spares the copy.
impl<'de> Deserialize<'de> for MemberName<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<MemberName<'de>, D::Error> {
        deserializer.deserialize_str(MemberNameVisitor)
    }
}

/// Reads one member name.
#[derive(Clone, Copy, Debug)]
struct MemberNameVisitor;

impl<'de> Visitor<'de> for MemberNameVisitor {
    type Value = MemberName<'de>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a member name")
    }

    fn visit_borrowed_str<E: DeserializeError>(self, name: &'de str) -> Result<MemberName<'de>, E> {
        Ok(MemberName::Borrowed(name))
    }

    fn visit_str<E: DeserializeError>(self, name: &str) -> Result<MemberName<'de>, E> {
        Ok(MemberName::Owned(name.to_owned()))
    }

    fn visit_string<E: DeserializeError>(self, name: String) -> Result<MemberName<'de>, E> {
        Ok(MemberName::Owned(name))
    }
}

/// Decodes a `T` from the members of an object through `T`'s own decoding.
#[derive(Debug)]
struct DeserializedFromMembers<T>(PhantomData<T>);

impl<'de, T: Deserialize<'de>> Visitor<'de> for DeserializedFromMembers<T> {
    type Value = T;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON object")
    }

    fn visit_map<A: MapAccess<'de>>(self, members: A) -> Result<T, A::Error> {
        T::deserialize(MapAccessDeserializer::new(members))
    }
}
