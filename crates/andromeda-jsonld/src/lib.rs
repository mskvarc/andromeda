//! How Andromeda resolves JSON-LD contexts and reshapes documents against
//! them.
//!
//! NGSI-LD is a linked-data API: ETSI GS CIM 009 v1.9.1 clause 5.5.7 says
//! every request and response carries an `@context`, clause 4.3.6.8 says which
//! released core context a versionless reference means, and clause 5.13 makes
//! the contexts a broker has resolved a resource of their own. Everything that
//! follows from those three is here — which contexts apply to one operation
//! ([`context_spec`]), where a context comes from ([`loader`],
//! [`context_registry`], [`core_context`]), and what expanding or compacting a
//! document against it produces ([`engine`]).
//!
//! This is a capability, not a policy. Nothing here decides what a client is
//! told, which resource is being addressed, or how a failure is categorised:
//! [`error::JsonLdError`] keeps a context-loading failure's URL, status and
//! reason as separate fields precisely so the caller can categorise it
//! without re-parsing a message.
//!
//! Two ports point outward and are declared here because this crate calls
//! them: [`context_cache::ContextCache`], the durable cache a broker may
//! attach, and `andromeda_ngsi_ld_query::term_expander::TermExpander`, which
//! this crate implements in [`term_expansion`] so the query language never
//! names JSON-LD.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod compaction;
pub mod config;
pub mod context_cache;
pub mod context_registry;
pub mod context_spec;
pub mod context_validation;
pub mod core_context;
pub mod core_context_precedence;
pub mod document_context;
pub mod engine;
pub mod error;
pub mod hit_counter;
pub mod loader;
pub mod term_expansion;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-vocabulary",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-ngsi-ld-representation",
    ],
    allow_production_dependencies: ["confique", "reqwest"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
