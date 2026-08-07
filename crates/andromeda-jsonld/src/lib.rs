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
mod boundary {
    /// The layer rule this crate is on the inside of.
    ///
    /// Asserted here rather than by a central test so the failure names the
    /// crate that broke the rule, and so a crate added to the workspace cannot
    /// be forgotten by a table it was never added to. The rule itself is read
    /// once, in `andromeda-layer-guard`, which every crate carries as a
    /// dev-dependency and which declares nothing of its own.
    /// Resolving a context needs an HTTP client and a cache, so `reqwest`,
    /// `moka` and `confique` are this crate's to declare — it is a capability,
    /// not a specification crate. What it must never reach is a store, a
    /// resource, or the web framework: the moment it does, resolving a context
    /// and answering a request are one thing again.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// In-workspace crates this one may name, in any dependency table.
        /// Anything else is an upward or sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-layer-guard",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-query",
            "andromeda-ngsi-ld-representation",
            "andromeda-ngsi-ld-vocabulary",
        ];

        // The engine resolves contexts over HTTP and reads its pinned core URL
        // from configuration, so it ships `reqwest` and `confique`.
        let forbidden = andromeda_layer_guard::manifest::baseline_forbidden_except(&["confique", "reqwest"]);
        andromeda_layer_guard::manifest::assert_layer(env!("CARGO_MANIFEST_DIR"), &forbidden, ALLOWED_IN_WORKSPACE);

        // Section 8 restricts the model→representation edge to tests: this crate
        // builds its fastpath-vs-processor equivalence tests against
        // `andromeda-ngsi-ld-representation` but must never ship it. The
        // allow-list above permits an edge in any table, so the dev-only
        // restriction is pinned separately — promoting it to a normal
        // dependency fails here.
        andromeda_layer_guard::manifest::assert_dev_only(env!("CARGO_MANIFEST_DIR"), "andromeda-ngsi-ld-representation");
    }
}

#[cfg(test)]
mod dialect {
    /// No storage dialect may be written here, in any form.
    ///
    /// The manifest guard above reads dependency names, which is exactly what a
    /// dialect emitted as a string constant slips past: a crate can spell an
    /// entire SQL predicate without declaring `sea-query`.
    #[test]
    fn no_source_file_in_this_crate_emits_a_storage_dialect() {
        andromeda_layer_guard::dialect::assert_no_dialect(env!("CARGO_MANIFEST_DIR"));
    }
}
