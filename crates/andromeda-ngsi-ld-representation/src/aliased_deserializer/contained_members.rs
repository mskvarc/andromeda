//! The members of an object written under a context that gives some of its
//! terms a container, read without the containers.
//!
//! Compaction writes a member under a term with a container in the shape the
//! container gives (JSON-LD 1.1 section 4.6), and a request is read "according
//! to the @context" (clause 5.5.4), so the data is the same whichever shape
//! wrote it: an array of one under a `@set` term is its one element, an object
//! of one key under a map container is the value under that key, and the
//! members under an alias of `@nest` belong to the object holding it. A map of
//! several keys is the array of its values in written order, the elements of a
//! value that is an array taking its place. Only a member under such a term is
//! held as the slice of text it is, so a document of a context without
//! containers never reaches this reader.

use crate::{
    aliased_deserializer::{Place, aliased_visitor::AliasedSeed, piece_sequence::PieceSequence},
    compact_node::context_reading::ContextReading,
    container_terms::TermContainer,
    core_term_aliases::Standing,
};
use andromeda_json_literal::piece::{JsonPiece, JsonString};
use serde::de::{DeserializeSeed, Error as DeserializeError, MapAccess, value::CowStrDeserializer};
use serde_json::{Deserializer as JsonReader, value::RawValue};
use std::{borrow::Cow, cell::Cell, collections::VecDeque};

/// Where the value of the member last named is read from.
#[derive(Debug)]
enum Delivery<'de> {
    /// The text the members are read from, still to be read.
    Streamed(Standing),
    /// A value already held as the slice of text it is.
    Held(JsonPiece<'de>, Standing),
    /// The values of a map container, read as the elements of one array.
    Values(Vec<JsonPiece<'de>>, Standing),
}

/// A member written under a term, with its value not yet read.
#[derive(Debug, Clone, Copy)]
enum Written<'de> {
    /// The value is next in the text the members are read from.
    Streamed,
    /// The value was held when the member it was nested under was.
    Held(JsonPiece<'de>),
}

/// The members of an object, each read without the container of its term and
/// under the core term its name stands for.
#[derive(Debug)]
pub struct ContainedMembers<'a, 'de, A> {
    /// The members as written.
    members: A,
    /// The aliases and containers each member is read through.
    reading: ContextReading<'a>,
    /// The members of the objects written under `@nest` aliases, which come
    /// before the members still to be read from the text.
    nested: VecDeque<(Cow<'de, str>, JsonPiece<'de>)>,
    /// Where the value of the member last named is read from.
    delivery: Delivery<'de>,
}

impl<'a, 'de, A> ContainedMembers<'a, 'de, A> {
    /// The `members` of an object, read through `reading`.
    #[must_use]
    pub const fn new(members: A, reading: ContextReading<'a>) -> ContainedMembers<'a, 'de, A> {
        ContainedMembers {
            members,
            reading,
            nested: VecDeque::new(),
            delivery: Delivery::Streamed(Standing::Node),
        }
    }

    /// Put `members`, those of an object written under a `@nest` alias, before
    /// every member still to be read.
    fn nest(&mut self, members: Vec<(Cow<'de, str>, JsonPiece<'de>)>) {
        members.into_iter().rev().for_each(|member| self.nested.push_front(member));
    }
}

impl<'de, A: MapAccess<'de>> ContainedMembers<'_, 'de, A> {
    /// The next member written, whichever place it is read from.
    fn next_written(&mut self) -> Result<Option<(Cow<'de, str>, Written<'de>)>, A::Error> {
        if let Some((name, piece)) = self.nested.pop_front() {
            return Ok(Some((name, Written::Held(piece))));
        }
        Ok(self.members.next_key::<JsonString<'de>>()?.map(|JsonString(name)| (name, Written::Streamed)))
    }

    /// The value of a member written as `written`, held as the slice of text
    /// it is.
    fn hold(&mut self, written: Written<'de>) -> Result<JsonPiece<'de>, A::Error> {
        match written {
            Written::Streamed => Ok(JsonPiece::new(self.members.next_value::<&'de RawValue>()?)),
            Written::Held(piece) => Ok(piece),
        }
    }

    /// The value of a member under a term with `container`, read without it:
    /// `None` when the member only stood for the members it nests.
    fn uncontained(&mut self, container: Option<TermContainer>, written: Written<'de>) -> Result<Option<Delivered<'de>>, A::Error> {
        let Some(container) = container else {
            return Ok(Some(Delivered::Written(written)));
        };
        let piece = self.hold(written)?;
        match container {
            TermContainer::Nest => match piece.members() {
                Some(members) => {
                    self.nest(members);
                    Ok(None)
                }
                None => Ok(Some(Delivered::Piece(piece))),
            },
            TermContainer::Set => Ok(Some(Delivered::Piece(single_element(piece).unwrap_or(piece)))),
            TermContainer::Map => Ok(Some(map_values(piece))),
        }
    }
}

/// What the value of a member is read as, once its container is undone.
#[derive(Debug)]
enum Delivered<'de> {
    /// The value as it was written.
    Written(Written<'de>),
    /// The value, held as the slice of text it is.
    Piece(JsonPiece<'de>),
    /// The values of a map of several keys, the elements of one array.
    Values(Vec<JsonPiece<'de>>),
}

/// The one element of the array `piece` is, when it holds exactly one.
fn single_element(piece: JsonPiece<'_>) -> Option<JsonPiece<'_>> {
    let mut elements = piece.elements()?;
    if elements.len() == 1 { elements.pop() } else { None }
}

/// The value of a member under a map container: the value under the one key of
/// a map of one, the values of a map of several, and any other value as
/// written.
fn map_values(piece: JsonPiece<'_>) -> Delivered<'_> {
    let Some(mut members) = piece.members() else {
        return Delivered::Piece(piece);
    };
    match members.len() {
        0 => Delivered::Piece(piece),
        1 => members.pop().map_or(Delivered::Piece(piece), |(_, value)| Delivered::Piece(value)),
        _ => Delivered::Values(
            members
                .into_iter()
                .flat_map(|(_, value)| value.elements().unwrap_or_else(|| vec![value]))
                .collect(),
        ),
    }
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for ContainedMembers<'_, 'de, A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, A::Error> {
        loop {
            let Some((written, value)) = self.next_written()? else {
                return Ok(None);
            };
            let container = self.reading.containers().container_of(&written);
            let Some(delivered) = self.uncontained(container, value)? else {
                continue;
            };
            let standing = Cell::new(Standing::Node);
            let name = Place::Name(&standing).read(self.reading.aliases(), written);
            self.delivery = match delivered {
                Delivered::Written(Written::Streamed) => Delivery::Streamed(standing.get()),
                Delivered::Written(Written::Held(piece)) | Delivered::Piece(piece) => Delivery::Held(piece, standing.get()),
                Delivered::Values(pieces) => Delivery::Values(pieces, standing.get()),
            };
            return seed.deserialize(CowStrDeserializer::new(name)).map(Some);
        }
    }

    fn next_value_seed<S: DeserializeSeed<'de>>(&mut self, seed: S) -> Result<S::Value, A::Error> {
        match &self.delivery {
            Delivery::Streamed(standing) => self.members.next_value_seed(AliasedSeed::new(seed, self.reading, Place::Value(*standing))),
            Delivery::Held(piece, standing) => {
                let mut reader = JsonReader::from_str(piece.text());
                AliasedSeed::new(seed, self.reading, Place::Value(*standing))
                    .deserialize(&mut reader)
                    .map_err(DeserializeError::custom)
            }
            Delivery::Values(pieces, standing) => AliasedSeed::new(seed, self.reading, Place::Value(*standing))
                .deserialize(PieceSequence::new(pieces))
                .map_err(DeserializeError::custom),
        }
    }
}
