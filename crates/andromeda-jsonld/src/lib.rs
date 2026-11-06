//! How Andromeda resolves JSON-LD contexts and reshapes documents against
//! them.
//!
//! NGSI-LD is a linked-data API: ETSI GS CIM 009 v1.9.1 clause 5.5.7 defines
//! how terms are expanded and compacted against an `@context`, clause 4.4 says
//! which released core context a versionless reference means, and clause 5.13 makes
//! the contexts a broker has resolved a resource of their own. Everything that
//! follows from those three is here: which contexts apply to one operation
//! ([`context_spec`]), where a context comes from ([`loader`],
//! [`context_registry`], [`core_context`], [`embedded_context`]), and what
//! expanding or compacting a document against it produces ([`engine`]). Where
//! a request named its
//! context -- one `Link` header, its body, or nowhere -- is
//! [`request_context::RequestContext`].
//!
//! This is a capability, not a policy. Nothing here decides what a client is
//! told, which resource is being addressed, or how a failure is categorised:
//! [`error::JsonLdError`] keeps a context-loading failure's URL, status and
//! reason as separate fields precisely so the caller can categorise it
//! without re-parsing a message.
//!
//! Clause 5.13.2.3's rule that everything outside a referenced context
//! object's `@context` subtree is discarded belongs to no single caller, so
//! [`context_subtree`] owns it for the resolver and the Context Registry alike.
//!
//! Which `@context` a response or a notification names (clauses 5.2.3 and
//! 4.4) depends on what the documents a request named leave in force last,
//! which only the resolver knows, so [`presented_context`] states it from the
//! [`context_tail`] the resolver records for every document it holds.
//!
//! The clause 5.5.7 admission of a request's user `@context` is stated here for
//! the same reason: whether it loads, and whether it declares a JSON-LD Scoped
//! Context, is decidable only once every context it names has been resolved,
//! which is this crate's capability and no caller's.
//! [`user_context_admission`] holds it, and its types make it the one way a
//! context a client submitted becomes one an operation may read under.
//!
//! Which terms of a processed context stand for core NGSI-LD terms is a
//! property of that context alone, so [`core_term_aliases`] reads it once for
//! the request body reader and the compacted-entity reader alike, and
//! [`container_terms`] reads which of its terms reshape what is compacted under
//! them.
//!
//! Four ports point outward and are declared here because this crate or its
//! callers reach through them: [`context_cache::ContextCache`], the durable
//! cache a broker may attach; [`compound_context_host::CompoundContextHost`],
//! the store a compound request `@context` is hosted in so one `Link` header can
//! name it; [`hosted_context_recognizer::HostedContextRecognizer`], which tells
//! the loader which references name one of the tenant's own stored `@context`s;
//! and the query language's term expander, which this crate implements in
//! [`term_expansion`] so the query language never names JSON-LD. Its inverse,
//! the term compactor, is implemented by the compaction crate, which compacts
//! names against a processed context.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod batch_contexts;
pub mod compound_context_host;
pub mod configured_engine;
pub mod container_terms;
pub mod context_cache;
pub mod context_graph;
pub mod context_registry;
pub mod context_spec;
pub mod context_subtree;
pub mod context_tail;
pub mod context_term;
pub mod context_validation;
pub mod context_value;
pub mod core_context;
pub mod core_context_precedence;
pub mod core_term_aliases;
pub mod document_context;
pub mod embedded_context;
pub mod engine;
pub mod error;
pub mod hit_counter;
pub mod hosted_context_recognizer;
pub mod loader;
pub mod operation_hit_scope;
pub mod presented_context;
pub mod request_context;
pub mod term_expansion;
/// Fixture documents for unit tests.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
pub(crate) mod test_documents;
pub mod user_context_admission;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration",
        "andromeda-http-headers",
        "andromeda-json-literal",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-outbound-http",
        "andromeda-persistence-failure",
        "andromeda-retry",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["reqwest", "tokio"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
