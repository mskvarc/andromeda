//! JSON-LD term coercion and container semantics used by the batch fast path.

use iri_rs::IriBuf;

/// The `@type` coercion a term declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TermCoercion {
    /// `@type: DateTime`.
    DateTime,
    /// `@type: @id`.
    Id,
    /// `@type: @vocab`.
    Vocab,
    /// `@type: @json`.
    Json,
    /// A user-declared datatype IRI.
    TypeIri(IriBuf),
}

/// The `@container` a term declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TermContainer {
    /// `@container: @list`.
    List,
    /// `@container: @language`.
    Language,
    /// `@container: @index`.
    Index,
    /// `@container: @set`.
    Set,
}
