//! Why the term walk hands a document to the JSON-LD processor.
//!
//! The walk decides each key and value of a compacted document one term at a
//! time. A context can make compaction depend on more than one term (a scoped
//! `@context` changes the active context below a property or a type, a `@nest`
//! moves a property into another object, a map container keys the values by a
//! part of them), and the walk does not reproduce those. Meeting one is not a
//! failure: the document is compacted by the processor instead, which is the
//! authority on every case (JSON-LD 1.1 API section 6.1).

use thiserror::Error;

/// A compaction decision the term walk leaves to the processor.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub(crate) enum Undecidable {
    /// A selected property term carries a scoped `@context` (JSON-LD 1.1
    /// section 4.1.8).
    #[error("a selected property term carries a scoped context")]
    PropertyScopedContext,
    /// A compacted `@type` value is a term carrying a type-scoped `@context`.
    #[error("a compacted type is a term carrying a type-scoped context")]
    TypeScopedContext,
    /// A selected property term nests its values under `@nest`.
    #[error("a selected property term nests its values")]
    NestedTerm,
    /// A selected property term has an `@index`, `@id`, `@type` or `@graph`
    /// container.
    #[error("a selected property term has a map container the walk does not write")]
    MapContainer,
    /// An IRI could only be written out in full and would then read back as a
    /// compact IRI (JSON-LD 1.1 API section 6.2, IRI confused with prefix).
    #[error("an IRI is confused with a prefix")]
    ConfusedIri,
    /// The context does not compact `GeoJSON` `coordinates` to the core
    /// context's `@list` term.
    #[error("the context does not compact GeoJSON coordinates to the core coordinates term")]
    GeometryTerms,
    /// Two values of one property compact into one key with shapes that do
    /// not merge.
    #[error("two values compact into one key with shapes that do not merge")]
    CollidingEntry,
    /// A value of the model has no JSON-LD form the walk can build, such as a
    /// datatype that is not an IRI.
    #[error("a value has no JSON-LD form the walk can build")]
    UnrepresentableValue,
}
