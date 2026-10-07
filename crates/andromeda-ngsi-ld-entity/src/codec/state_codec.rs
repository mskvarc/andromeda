//! The canonical codec of [`EntityState`]: the stored document,
//! `[{"@id": .., "@type": [..], <members>}]`, to and from bytes.
//!
//! Decoding reads the written document and converts it with the same rules a
//! create applies, so a document the store could never hold (one carrying the
//! NGSI-LD Null, a repeated `datasetId`, a `GeometryCollection`) is refused
//! with a typed [`EntityRefusal`] naming the attribute and the dataset.

use crate::{
    codec::{decode_context::DecodeContext, entity_decode::DocumentSeed, entity_encode::StateNodeOut},
    document::EntityDocument,
    entity_state::EntityState,
    refusal::EntityRefusal,
};
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
    de::{DeserializeSeed, Error},
};
use std::{io::Write, iter::once};

impl Serialize for EntityState {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(once(StateNodeOut(self)))
    }
}

impl<'de> Deserialize<'de> for EntityState {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<EntityState, D::Error> {
        let context = DecodeContext::default();
        let document = DocumentSeed { context: &context }.deserialize(deserializer)?;
        EntityState::try_from(document).map_err(D::Error::custom)
    }
}

impl EntityState {
    /// Decode the stored document `bytes`.
    ///
    /// # Errors
    ///
    /// Returns the [`EntityRefusal`] of a text that is not JSON, of a document
    /// not shaped as an expanded entity, or of one state cannot hold.
    pub fn from_slice(bytes: &[u8]) -> Result<EntityState, EntityRefusal> {
        EntityState::try_from(EntityDocument::from_slice(bytes)?)
    }

    /// Encode the stored document.
    ///
    /// # Errors
    ///
    /// Returns the writer's failure; encoding the model itself cannot fail.
    pub fn to_writer<W: Write>(&self, writer: W) -> Result<(), serde_json::Error> {
        serde_json::to_writer(writer, self)
    }

    /// Encode the stored document into a byte vector.
    ///
    /// # Errors
    ///
    /// None in practice; the signature carries the serializer's error type.
    pub fn to_vec(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
}
