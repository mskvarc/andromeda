//! What the typed walk answers: the expanded document, or why the request
//! goes to the JSON-LD processor instead.

use crate::decline::Decline;

/// The outcome of expanding one request without the processor.
#[derive(Clone, Debug, PartialEq)]
pub enum TypedExpansion<T> {
    /// The request expanded; equal to what the processor's expansion reads as.
    Expanded(T),
    /// The request uses something the walk does not model.
    Declined(Decline),
}
