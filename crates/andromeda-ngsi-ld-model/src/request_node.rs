//! A resource read from the compact node of a request body.
//!
//! A body is read through the terms of the context it was written under:
//! the containers of its terms are undone first, and the aliases of core terms
//! are read after (clause 5.5.7). Undoing containers costs a copy of every
//! level of the document, and a context that gives its terms none has nothing
//! to undo, so such a body is read through its aliases alone.

use crate::request_body::DecodeBounds;
use andromeda_json_literal::piece::JsonPiece;
use andromeda_ngsi_ld_representation::{
    compact_node::{CompactNode, aliased_node::AliasedNode, bounded_piece::BoundedPiece, context_reading::ContextReading},
    value_budget::ValueBudget,
};
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
    read_through(JsonPiece::new(raw), reading)
}

/// Read a `T` as [`read_request_node`] does, each geometry and each growing
/// member of the body recorded within `bounds` (clause 6.3.2).
///
/// # Errors
///
/// As [`read_request_node`].
pub(crate) fn read_bounded_request_node<T: NodeRead>(text: &[u8], reading: ContextReading<'_>, bounds: DecodeBounds) -> Result<T, serde_json::Error> {
    let raw: &RawValue = serde_json::from_slice(text)?;
    let values = ValueBudget::new(bounds.entity_values());
    read_through(BoundedPiece::new(JsonPiece::new(raw), bounds.geometry_positions(), &values), reading)
}

/// Read a `T` from `node` through `reading`.
fn read_through<'n, T: NodeRead, N: CompactNode<'n, Unreadable = Infallible>>(node: N, reading: ContextReading<'n>) -> Result<T, serde_json::Error> {
    if reading.containers().is_empty() {
        T::read_node(AliasedNode::new(node, reading.aliases()))
    } else {
        T::read_node(reading.read(node))
    }
}
