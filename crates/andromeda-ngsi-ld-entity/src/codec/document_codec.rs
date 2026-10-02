//! The canonical codec of written documents: an [`EntityDocument`] and the
//! bare attribute fragment of clauses 5.6.4 and 5.6.19, to and from their
//! expanded JSON-LD text. Written documents are persisted as they are in the
//! durable queues and the temporal hand-off, so they round-trip like state.

use crate::{
    codec::{
        decode_context::DecodeContext,
        document_encode::InstanceDocumentOut,
        entity_decode::{DocumentSeed, FragmentSeed},
        entity_encode::DocumentNodeOut,
    },
    document::{EntityDocument, instance_document::InstanceDocument},
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeSeed};
use std::{io::Write, iter::once};

impl Serialize for EntityDocument {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(once(DocumentNodeOut(self)))
    }
}

impl<'de> Deserialize<'de> for EntityDocument {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<EntityDocument, D::Error> {
        let context = DecodeContext::default();
        DocumentSeed { context: &context }.deserialize(deserializer)
    }
}

impl EntityDocument {
    /// Decode the expanded document `bytes`.
    ///
    /// # Errors
    ///
    /// Returns the [`EntityRefusal`] of a text that is not JSON or of a
    /// document not shaped as an expanded entity.
    pub fn from_slice(bytes: &[u8]) -> Result<EntityDocument, EntityRefusal> {
        let context = DecodeContext::default();
        let mut deserializer = serde_json::Deserializer::from_slice(bytes);
        DocumentSeed { context: &context }
            .deserialize(&mut deserializer)
            .and_then(|document| deserializer.end().map(|()| document))
            .map_err(|error| context.into_refusal(&error))
    }

    /// Encode the expanded document.
    ///
    /// # Errors
    ///
    /// Returns the writer's failure.
    pub fn to_writer<W: Write>(&self, writer: W) -> Result<(), serde_json::Error> {
        serde_json::to_writer(writer, self)
    }

    /// Encode the expanded document into a byte vector.
    ///
    /// # Errors
    ///
    /// None in practice; the signature carries the serializer's error type.
    pub fn to_vec(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
}

impl InstanceDocument {
    /// Decode the expanded attribute fragment `bytes` written for `attribute`
    /// (clauses 5.6.4, 5.6.19): one instance, bare or in a one-element array.
    ///
    /// # Errors
    ///
    /// Returns the [`EntityRefusal`] of a text that is not JSON or of a
    /// fragment not shaped as an expanded attribute instance.
    pub fn from_slice(attribute: &AttributeName, bytes: &[u8]) -> Result<InstanceDocument, EntityRefusal> {
        let context = DecodeContext::default();
        let mut deserializer = serde_json::Deserializer::from_slice(bytes);
        FragmentSeed { context: &context, attribute }
            .deserialize(&mut deserializer)
            .and_then(|instance| deserializer.end().map(|()| instance))
            .map_err(|error| context.into_refusal(&error))
    }

    /// Encode the fragment as a one-element array.
    ///
    /// # Errors
    ///
    /// None in practice; the signature carries the serializer's error type.
    pub fn to_vec(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(&[InstanceDocumentOut(self)])
    }
}
