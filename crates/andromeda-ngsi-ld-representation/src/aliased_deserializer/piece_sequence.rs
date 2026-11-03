//! The values of a map container, delivered as the elements of one array.
//!
//! Each element stays the slice of text it was written as, so a value the
//! resource captures as JSON of its own is handed over as written.

use andromeda_json_literal::piece::JsonPiece;
use serde::{
    de::{DeserializeSeed, Deserializer, SeqAccess, Visitor},
    forward_to_deserialize_any,
};
use serde_json::Deserializer as JsonReader;
use std::slice::Iter;

/// A deserializer over pieces of JSON text that delivers them as one array.
#[derive(Debug)]
pub(crate) struct PieceSequence<'p, 'de> {
    /// The pieces still to be delivered, in order.
    pieces: Iter<'p, JsonPiece<'de>>,
}

impl<'p, 'de> PieceSequence<'p, 'de> {
    /// The array whose elements are `pieces`.
    pub(crate) fn new(pieces: &'p [JsonPiece<'de>]) -> PieceSequence<'p, 'de> {
        PieceSequence { pieces: pieces.iter() }
    }
}

impl<'de> Deserializer<'de> for PieceSequence<'_, 'de> {
    type Error = serde_json::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, serde_json::Error> {
        visitor.visit_seq(self)
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, serde_json::Error> {
        visitor.visit_some(self)
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

impl<'de> SeqAccess<'de> for PieceSequence<'_, 'de> {
    type Error = serde_json::Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>, serde_json::Error> {
        self.pieces
            .next()
            .map(|piece| seed.deserialize(&mut JsonReader::from_str(piece.text())))
            .transpose()
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.pieces.len())
    }
}
