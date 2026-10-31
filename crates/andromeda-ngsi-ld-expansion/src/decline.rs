//! Why the typed walk leaves a request to the JSON-LD processor.
//!
//! The walk models the JSON-LD 1.1 expansion algorithm for the shapes NGSI-LD
//! requests take. A context feature or a document shape outside them is not
//! guessed at: the walk declines, and the processor, which is the authority on
//! expansion (clause 5.5.7), expands the request instead, and reports the
//! failure of a document that is not valid JSON-LD.

use strum::{Display, IntoStaticStr};

/// What the typed walk does not model.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "kebab-case")]
pub enum Decline {
    /// An `@context` inside the document, which the processor processes on
    /// top of the request's context (clause 5.5.7 refuses it in client
    /// requests before expansion).
    EmbeddedContext,
    /// A keyword member the walk does not model: `@graph`, `@included`,
    /// `@reverse`, `@nest`, `@index`, or `@language`/`@direction` on a node.
    UnsupportedKeyword,
    /// A term defined as a reverse property (`@reverse`).
    ReverseProperty,
    /// A term whose container is `@index`, `@id`, `@type` or `@graph`, or a
    /// container on a member whose value the walk read from the typed model.
    UnsupportedContainer,
    /// A type-scoped context: a type term that declares an `@context`.
    TypeScopedContext,
    /// A property-scoped context met inside another scoped context, or one
    /// that does not process.
    NestedScopedContext,
    /// A scalar written under a term that declares a property-scoped context.
    ScopedScalar,
    /// A context that does not propagate to nested node objects
    /// (`"@propagate": false`).
    NonPropagatedContext,
    /// A shape JSON-LD 1.1 expansion refuses (an `@id` that is no string, an
    /// invalid value object, colliding keywords, ...); the processor reports
    /// the precise error.
    MalformedJsonLd,
    /// A member the typed compact model carries only in responses (`entity`,
    /// `entityList`), left to the request validation that refuses it.
    OutputOnlyMember,
    /// A typed member of the compact model did not serialize as JSON.
    CompactSerialization,
    /// The values the write expands to passed the entity value limit: the walk
    /// stops, and the expander refuses the payload (clause 6.3.2) rather than
    /// handing it to the processor.
    ValueLimit,
}

#[cfg(test)]
mod tests {
    use crate::decline::Decline;

    #[test]
    fn a_decline_reads_as_its_kebab_case_name() {
        pretty_assertions::assert_eq!(Decline::TypeScopedContext.to_string(), "type-scoped-context");
        pretty_assertions::assert_eq!(<&str>::from(Decline::MalformedJsonLd), "malformed-json-ld");
    }
}
