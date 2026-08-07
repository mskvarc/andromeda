//! The NGSI-LD query languages: their grammars, their ASTs and their compiled forms.
//!
//! ETSI GS CIM 009 v1.9.1 defines a family of query facilities — `q` (clause
//! 4.9), `georel` (4.10), `scopeQ` (4.18), type selection (4.17), `orderBy`
//! (4.23), projection (6.3.15) and the temporal window (4.5.19) — that a broker
//! must read identically wherever they appear. This crate owns all of them: each
//! concept's grammar, its AST and its compiled model live in one directory, so
//! a change to one concept touches one directory rather than several pipeline
//! stages.
//!
//! Modules are therefore named after NGSI-LD concepts, never after stages. How
//! a compiled query is rendered as SQL is deliberately absent: that is a
//! property of a backend, and it lives in `andromeda-postgresql-query` and the
//! two statement crates above it. Nothing here names a database.
//!
//! Term expansion is likewise inverted rather than depended upon: compilation
//! that needs a term expanded against an `@context` asks a
//! [`term_expander::TermExpander`], which the JSON-LD engine implements.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod csr;
pub mod dataset;
pub mod entity;
pub mod entity_selection;
pub mod error;
pub mod filter;
pub mod geo;
pub mod identifier;
pub mod ordering;
pub mod pagination;
pub mod pattern;
pub mod projection;
pub mod scope;
pub mod temporal;
pub mod term_expander;
pub mod type_selection;

#[cfg(test)]
mod boundary {
    /// The layer rule this crate is on the inside of.
    ///
    /// Asserted here rather than by a central test so the failure names the
    /// crate that broke the rule, and so a crate added to the workspace cannot
    /// be forgotten by a table it was never added to. The rule itself is read
    /// once, in `andromeda-layer-guard`, which every crate carries as a
    /// dev-dependency and which declares nothing of its own.
    /// The data model is deliberately absent: ETSI GS CIM 009 puts query
    /// expressions *inside* resources (clauses 5.2.9, 5.2.12, 5.2.23), so the
    /// resources sit above the languages. An edge the other way is what forced
    /// `Subscription.q` to be a `String`.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// In-workspace crates this one may name, in any dependency table.
        /// Anything else is an upward or sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-layer-guard",
            "andromeda-ngsi-ld-geometry",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-representation",
            "andromeda-ngsi-ld-vocabulary",
        ];

        andromeda_layer_guard::manifest::assert_layer(
            env!("CARGO_MANIFEST_DIR"),
            andromeda_layer_guard::manifest::BASELINE_FORBIDDEN,
            ALLOWED_IN_WORKSPACE,
        );
    }
}

#[cfg(test)]
mod dialect {
    /// No storage dialect may be written here, in any form.
    ///
    /// The manifest guard above reads dependency names, which is exactly what a
    /// dialect emitted as a string constant slips past: a specification crate
    /// can spell an entire SQL predicate without declaring `sea-query`.
    #[test]
    fn no_source_file_in_this_crate_emits_a_storage_dialect() {
        andromeda_layer_guard::dialect::assert_no_dialect(env!("CARGO_MANIFEST_DIR"));
    }
}
