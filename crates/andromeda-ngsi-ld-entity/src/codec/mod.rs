//! The canonical codec: the model to and from the expanded JSON-LD document
//! the broker stores and exchanges (clause 5.5.7 expansion, unchanged).
//!
//! Decoding streams through serde visitors straight from bytes into the typed
//! model, a geometry's nested `@list`s straight into its positions; encoding
//! writes straight into a serializer. Neither goes through a generic JSON tree,
//! except inside a JSON literal, whose value is one by definition (clause
//! 4.5.24).

mod carrier_decode;
mod carrier_encode;
mod coordinate_list;
mod coordinates_decode;
mod decode_context;
mod document_codec;
#[cfg(test)]
mod document_codec_tests;
mod document_encode;
mod element;
mod entity_decode;
mod entity_encode;
mod geojson_members_codec;
pub(crate) mod geometry_decode;
#[cfg(test)]
mod geometry_decode_tests;
mod geometry_encode;
mod instance_decode;
mod instance_encode;
mod member_decode;
mod object_key;
mod public_encode;
mod raw_scalar;
mod state_codec;
#[cfg(test)]
mod state_codec_tests;
#[cfg(test)]
pub(crate) mod stored_document_fixtures;
mod value_encode;
