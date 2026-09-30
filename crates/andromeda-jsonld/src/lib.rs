//! How Andromeda resolves JSON-LD contexts and reshapes documents against
//! them.
//!
//! NGSI-LD is a linked-data API: ETSI GS CIM 009 v1.9.1 clause 5.5.7 defines
//! how terms are expanded and compacted against an `@context`, clause 4.4 says
//! which released core context a versionless reference means, and clause 5.13 makes
//! the contexts a broker has resolved a resource of their own. Everything that
//! follows from those three is here — which contexts apply to one operation
//! ([`context_spec`]), where a context comes from ([`loader`],
//! [`context_registry`], [`core_context`]), and what expanding or compacting a
//! document against it produces ([`engine`]). Where a request named its
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
//! One clause 5.5.7 rule is stated here for the same reason: whether the user
//! `@context` of a request declares a JSON-LD Scoped Context is decidable only
//! once every context it names has been resolved, which is this crate's
//! capability and no caller's. [`scoped_context_admission`] holds it; which
//! operations run it is the callers' decision.
//!
//! Four ports point outward and are declared here because this crate or its
//! callers reach through them: [`context_cache::ContextCache`], the durable
//! cache a broker may attach; [`compound_context_host::CompoundContextHost`],
//! the store a compound request `@context` is hosted in so one `Link` header can
//! name it; [`hosted_context_recognizer::HostedContextRecognizer`], which tells
//! the loader which references name one of the tenant's own stored `@context`s;
//! and `andromeda_ngsi_ld_query::term_expander::TermExpander`, which
//! this crate implements in [`term_expansion`] so the query language never
//! names JSON-LD. Its inverse, `andromeda_ngsi_ld_query::term_compactor::TermCompactor`,
//! is implemented in [`term_compaction`] for the same reason.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod attribute_fragment;
pub mod batch_contexts;
pub mod compaction;
pub mod compound_context_host;
pub mod config;
pub mod context_cache;
pub mod context_graph;
pub mod context_registry;
pub mod context_spec;
pub mod context_subtree;
pub mod context_tail;
pub mod context_term;
pub mod context_validation;
pub mod core_context;
pub mod core_context_precedence;
pub mod document_context;
pub mod engine;
pub mod error;
pub mod hit_counter;
pub mod hosted_context_recognizer;
pub mod loader;
pub mod operation_hit_scope;
pub mod presented_context;
pub mod request_context;
pub mod scoped_context_admission;
pub mod term_compaction;
pub mod term_expansion;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration-scalars",
        "andromeda-http-headers",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-persistence-failure",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-ngsi-ld-representation",
        "andromeda-test-mocks",
    ],
    allow_production_dependencies: ["reqwest", "tokio"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
