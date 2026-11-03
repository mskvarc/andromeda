//! Reading a request body straight from the JSON text it arrived as.
//!
//! An NGSI-LD request body is decoded from its text, never from a JSON tree
//! parsed first: a tree costs an allocation per value, and an Entity carrying
//! a geometry of a million positions is millions of values. Its top-level
//! `@context` member is not part of the resource: clause 6.3.5 has an
//! `application/ld+json` body name its context there, and the transport reads
//! it before the body is decoded, so the decoding skips it.
//!
//! The body is read "according to the @context" (clause 5.5.4): a member
//! written `@id` or `@type` (clause 4.4), or under a term the request's
//! context defines for a core term (clause 5.5.7), is read as that core term.
//! The decoding takes the [`ContextReading`] of the admitted context: a member
//! written in the shape the container of its term gives (JSON-LD 1.1 section
//! 4.6, clause 5.5.7) is read as the same member written without it. The
//! reading is applied through the one reader every compact document is read
//! with.

use crate::{
    attribute::member_reading::MemberReading,
    entity::{Entity, EntityPatch},
    node_refusal::NodeReadError,
    query::body::Query,
    request_node::{NodeRead, read_bounded_request_node, read_request_node},
    snapshot_clone::SnapshotCloneInput,
    snapshot_input::SnapshotInput,
    snapshot_patch::SnapshotPatch,
};
use andromeda_json_literal::piece::JsonString;
use andromeda_ngsi_ld_geometry::geometry_census::GeometryPositionLimit;
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_representation::{
    aliased_deserializer::AliasedDeserializer,
    compact_node::{CompactNode, context_reading::ContextReading, produced_node::ProducedNode},
};
use andromeda_ngsi_ld_vocabulary::keyword;
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
use std::{borrow::Cow, convert::Infallible, fmt, marker::PhantomData};

/// How much of a request body is recorded before the rest is skipped, so that
/// a body the broker will refuse for its size (clause 6.3.2) costs what the
/// bounds cost whatever its size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodeBounds {
    /// The positions one geometry may hold.
    geometry_positions: GeometryPositionLimit,
    /// The values one entity may hold.
    entity_values: JsonValueLimit,
}

impl DecodeBounds {
    /// Bounds for geometries of `geometry_positions` and entities of
    /// `entity_values`.
    #[must_use]
    pub const fn new(geometry_positions: GeometryPositionLimit, entity_values: JsonValueLimit) -> DecodeBounds {
        DecodeBounds {
            geometry_positions,
            entity_values,
        }
    }

    /// The positions one geometry may hold.
    #[must_use]
    pub const fn geometry_positions(self) -> GeometryPositionLimit {
        self.geometry_positions
    }

    /// The values one entity may hold.
    #[must_use]
    pub const fn entity_values(self) -> JsonValueLimit {
        self.entity_values
    }
}

/// A resource an NGSI-LD request body is decoded into.
pub trait RequestBody: DeserializeOwned {
    /// Decode the resource from the JSON text of a request body, skipping its
    /// top-level `@context` member, each member read under the core term
    /// `reading` says it stands for and without the container of its term.
    ///
    /// The result is the one the type's own decoding gives for the body with
    /// that member removed, its core terms spelled as such and its containers
    /// undone, read as `serde_json`'s streaming reader reads it.
    ///
    /// # Errors
    ///
    /// Returns the reader's failure, positioned in the text.
    fn from_request_text(text: &[u8], reading: ContextReading<'_>) -> Result<Self, serde_json::Error> {
        read_context_free(text, reading, DeserializedFromMembers::<Self>(PhantomData))
    }

    /// Decode the resource as [`RequestBody::from_request_text`] does, a
    /// geometry it carries recorded no further than a geometry of the bounds'
    /// positions can hold and the members of an entity that grow without bound
    /// recorded no further than the bounds' entity values (clause 6.3.2), so
    /// refusing an oversized geometry or entity costs what the bounds cost
    /// whatever its size.
    ///
    /// A resource that carries neither reads exactly as
    /// [`RequestBody::from_request_text`] reads it.
    ///
    /// # Errors
    ///
    /// Returns the reader's failure, positioned in the text.
    fn from_bounded_request_text(text: &[u8], reading: ContextReading<'_>, _bounds: DecodeBounds) -> Result<Self, serde_json::Error> {
        Self::from_request_text(text, reading)
    }
}

impl RequestBody for Query {}

impl RequestBody for SnapshotInput {}

impl RequestBody for SnapshotPatch {}

impl RequestBody for SnapshotCloneInput {}

impl<M: MemberReading> RequestBody for Entity<M> {
    /// Decode an Entity from the JSON text of a request body, read in place
    /// as a Context Producer wrote it.
    ///
    /// Each member is read from the slice of text it is, a `GeoProperty`'s
    /// geometry without any JSON tree, and every value the entity holds as
    /// JSON is admitted with its numbers in their canonical spelling. The
    /// members a producer may not supply are left out once their names are
    /// read as core terms, so no alias reaches a system member.
    fn from_request_text(text: &[u8], reading: ContextReading<'_>) -> Result<Entity<M>, serde_json::Error> {
        read_request_node(text, reading)
    }

    /// Decode an Entity as [`Entity::from_request_text`] does, each geometry
    /// and each growing member recorded within `bounds`.
    fn from_bounded_request_text(text: &[u8], reading: ContextReading<'_>, bounds: DecodeBounds) -> Result<Entity<M>, serde_json::Error> {
        read_bounded_request_node(text, reading, bounds)
    }
}

impl<M: MemberReading> NodeRead for Entity<M> {
    fn read_node<'n, N: CompactNode<'n, Unreadable = Infallible>>(node: N) -> Result<Entity<M>, serde_json::Error> {
        Entity::<M>::from_node(ProducedNode::new(node)).map_err(refused)
    }
}

impl<M: MemberReading> RequestBody for EntityPatch<M> {
    /// Decode a partial Entity from the JSON text of a request body, as
    /// [`Entity`] is decoded.
    fn from_request_text(text: &[u8], reading: ContextReading<'_>) -> Result<EntityPatch<M>, serde_json::Error> {
        read_request_node(text, reading)
    }

    /// Decode a partial Entity as [`EntityPatch::from_request_text`] does,
    /// each geometry and each growing member recorded within `bounds`.
    fn from_bounded_request_text(text: &[u8], reading: ContextReading<'_>, bounds: DecodeBounds) -> Result<EntityPatch<M>, serde_json::Error> {
        read_bounded_request_node(text, reading, bounds)
    }
}

impl<M: MemberReading> NodeRead for EntityPatch<M> {
    fn read_node<'n, N: CompactNode<'n, Unreadable = Infallible>>(node: N) -> Result<EntityPatch<M>, serde_json::Error> {
        EntityPatch::<M>::from_node(ProducedNode::new(node)).map_err(refused)
    }
}

/// Decode a `T` from the JSON text of a request body, its top-level `@context`
/// member skipped and no member read as a core term but `@id` and `@type`.
///
/// For a resource that holds the body as written and is read under the
/// context later, so that the context is applied once.
///
/// # Errors
///
/// Returns the reader's failure, positioned in the text.
pub(crate) fn read_as_written<T: DeserializeOwned>(text: &[u8]) -> Result<T, serde_json::Error> {
    read_context_free(text, ContextReading::keyword_spellings(), DeserializedFromMembers::<T>(PhantomData))
}

/// The reader's error for a body the compact model's reading refuses.
fn refused(error: NodeReadError<Infallible>) -> serde_json::Error {
    match error {
        NodeReadError::Refused(refused) => serde_json::Error::custom(refused),
        NodeReadError::Unreadable(never) => match never {},
    }
}

/// Read the object `text` holds with `visitor` through `reading`, its
/// `@context` member skipped.
///
/// # Errors
///
/// Returns the reader's failure, trailing content included.
fn read_context_free<'de, V: Visitor<'de>>(text: &'de [u8], reading: ContextReading<'_>, visitor: V) -> Result<V::Value, serde_json::Error> {
    let mut reader = JsonReader::from_slice(text);
    let read = AliasedDeserializer::new(&mut reader, reading).deserialize_map(ContextFree(visitor))?;
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
            match self.members.next_key::<JsonString<'de>>()? {
                None => return Ok(None),
                Some(JsonString(name)) if name == keyword::CONTEXT => {
                    self.members.next_value::<IgnoredAny>()?;
                }
                Some(JsonString(Cow::Borrowed(name))) => return seed.deserialize(BorrowedStrDeserializer::new(name)).map(Some),
                Some(JsonString(Cow::Owned(name))) => return seed.deserialize(StringDeserializer::new(name)).map(Some),
            }
        }
    }

    fn next_value_seed<S: DeserializeSeed<'de>>(&mut self, seed: S) -> Result<S::Value, A::Error> {
        self.members.next_value_seed(seed)
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
