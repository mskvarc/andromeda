//! How an NGSI-LD resource is rendered, as opposed to what it is.
//!
//! ETSI GS CIM 009 v1.9.1 separates a resource from its representation: clause
//! 4.5.1 names the normalized rendering, 4.5.4 the `keyValues`/simplified one
//! and the per-type `.3` subclauses the concise one; clause 4.8 says when the
//! system-generated attributes appear in one, and clause 4.5.23 says whether a
//! linked entity is nested or flat.
//! None of those decisions changes what an entity *is*, which is why they are
//! not members of the data model, and every one of them is read on both sides
//! of it — by the query languages that compile a request and by the renderers
//! that answer one.
//!
//! It sits below both: a representation option is expressible in terms of the
//! lexical layer alone, so nothing here names a resource, a query or a store.
//!
//! [`concise_reading`] says what a compact node written where an attribute
//! stands is under clause 4.5.2.3 and its siblings: an instance of which
//! family, a bare value, or a node they do not admit. Clause 4.5.2.3 lets a
//! *request* abbreviate an attribute; the typed model reads the abbreviation
//! as the attribute it stands for. [`attribute_type_validation`] says why a
//! node the reading refuses is rejected before it could be mistaken for a
//! value. The concise rendering writes only nodes the same reading reads back.
//!
//! The two member rules beside them say which members an attribute of a given
//! family may carry at all. [`family_member_admission`] holds the fourteen
//! "Prohibited" lists of clauses 4.5.2 through 4.5.24 as the one rule they
//! are, and [`proof_seal_admission`] the `ngsildproof` exception of clause
//! 4.5.2.2. They are stated here rather than beside a store or a service
//! because each is a statement about a representation and nothing else, and
//! because every layer that admits a written attribute has to agree on them.
//!
//! [`payload_budget`] counts what a written compact payload will cost once
//! expanded, before the expansion is attempted: what a representation costs is
//! read off the representation alone. [`attribute_nesting`] bounds how deeply
//! one attribute of it may nest, so that its stored expansion reads back.
//!
//! [`compact_key_admission`] is the fourth of those representation rules and
//! the only one about the keys a document is written with rather than which
//! members an attribute may carry: clause 5.5.7 admits a user `@context` at
//! the top level of an NGSI-LD document and nowhere else, and clause 4.6.4
//! refuses a key JSON-LD expansion would drop or rewrite past recovery.
//!
//! [`core_term_aliases`] is the one rule for a core term written under
//! another term: the clause 4.4 keyword spellings `@id` and `@type`, and the
//! terms a context defines for core terms (clause 5.5.7). Every reader of a
//! compact document applies it, a request body and a compacted entity alike:
//! [`aliased_deserializer`] applies it to a body decoded straight from its
//! text. [`container_terms`] is its companion for the shape compaction gives a
//! member written under a term with a container of the context's own, and
//! [`compact_node::context_reading`] reads a node through both.
//!
//! How a client *asks* for one is not here. Media-type negotiation, version
//! conformance and language ranges arrive as HTTP headers and are spelled by
//! the header grammars that carry them, so they live in
//! `andromeda-http-headers` beside those headers rather than being
//! split from them.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod aliased_deserializer;
pub mod attribute_nesting;
pub mod attribute_type_validation;
pub mod compact_key_admission;
pub mod compact_node;
pub mod concise_reading;
pub mod container_terms;
pub mod context_terms;
pub mod core_term_aliases;
pub mod embedded_context_admission;
pub mod family_member_admission;
pub mod join_mode;
pub mod linked_resolution_failure;
pub mod merge_representation;
pub mod payload_budget;
pub mod proof_seal_admission;
pub mod response_format;
pub mod sys_attrs;

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod written_json;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration",
        "andromeda-json-literal",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-vocabulary",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
