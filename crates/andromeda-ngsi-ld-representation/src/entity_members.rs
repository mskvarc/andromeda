//! The members of an entity that are not attributes (clause 5.2.4).
//!
//! Every representation renderer needs to tell an entity's own members apart
//! from its attributes: the members pass through a normalisation, conciseness
//! or simplification transform untouched, while the attributes are rewritten.
//! One table owns that distinction so the three renderers cannot drift on which
//! keys are members — a `keyValues` rendering that treats `createdAt` as an
//! attribute and a `concise` one that does not would answer the same entity two
//! ways.
//!
//! The expanded `@id`/`@type` forms are listed alongside the compact `id`/`type`
//! so the table serves the normalisation path (which sees expanded JSON) as well
//! as the compaction and simplification paths (which see compact JSON); a key
//! that a given stage never produces simply never matches.

/// The entity members clause 5.2.4 defines, which are never attributes.
pub const ENTITY_MEMBERS: &[&str] = &[
    "id",
    "type",
    "scope",
    "@context",
    "@id",
    "@type",
    "expiresAt",
    "createdAt",
    "modifiedAt",
    "deletedAt",
];
