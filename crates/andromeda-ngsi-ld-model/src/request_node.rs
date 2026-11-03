//! A resource read from the compact node of a request body.
//!
//! A body is read through the terms of the context it was written under:
//! the containers of its terms are undone first, and the aliases of core terms
//! are read after (clause 5.5.7). Undoing containers costs a copy of every
//! level of the document, and a context that gives its terms none has nothing
//! to undo, so such a body is read through its aliases alone.

use andromeda_json_literal::piece::JsonPiece;
use andromeda_ngsi_ld_representation::compact_node::{CompactNode, aliased_node::AliasedNode, context_reading::ContextReading};
use serde_json::value::RawValue;
use std::convert::Infallible;

/// A resource a request body is read into from a compact node.
pub(crate) trait NodeRead: Sized {
    /// Read the resource from `node`, the body as a Context Producer wrote it.
    ///
    /// # Errors
    ///
    /// Returns the reader's failure for a node the resource cannot be read
    /// from.
    fn read_node<'n, N: CompactNode<'n, Unreadable = Infallible>>(node: N) -> Result<Self, serde_json::Error>;
}

/// Read a `T` from the JSON text of a request body, in place, through
/// `reading`.
///
/// # Errors
///
/// Returns the reader's failure for text that is no JSON value, and the
/// failure of [`NodeRead::read_node`] otherwise.
pub(crate) fn read_request_node<T: NodeRead>(text: &[u8], reading: ContextReading<'_>) -> Result<T, serde_json::Error> {
    let raw: &RawValue = serde_json::from_slice(text)?;
    let piece = JsonPiece::new(raw);
    if reading.containers().is_empty() {
        T::read_node(AliasedNode::new(piece, reading.aliases()))
    } else {
        T::read_node(reading.read(piece))
    }
}
