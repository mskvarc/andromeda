//! How an NGSI-LD broker reports a failure.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.2 fixes twelve error categories, each with
//! a canonical type IRI, an HTTP status and a title, and clauses 5.2.16 and
//! 6.17 give batch operations a per-entity result shape built from the same
//! categories. Both are wire contracts: a client reads them, a Context Source
//! writes them, and a broker forwarding an operation has to parse one back.
//!
//! What is *not* here is Andromeda's own error type, or any conversion from it.
//! A category is what the specification says a failure is called; deciding
//! which category a particular internal failure falls into is broker policy and
//! belongs to the layer that holds that failure. Nothing here names a
//! framework, so the same shapes serve the server answering a request and the
//! forwarding client reading someone else's answer.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod batch_operation;
pub mod error_type;
pub mod problem_detail;

#[cfg(test)]
mod boundary {
    /// The layer rule this crate is on the inside of.
    ///
    /// Asserted here rather than by a central test so the failure names the
    /// crate that broke the rule, and so a crate added to the workspace cannot
    /// be forgotten by a table it was never added to. The rule itself is read
    /// once, in `andromeda-layer-guard`, which every crate carries as a
    /// dev-dependency and which declares nothing of its own.
    /// Batch problem details carry the validated URI types defined by the
    /// lexical layer, but name no resource model or higher-level policy crate.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// In-workspace crates this one may name, in any dependency table.
        /// Anything else is an upward or sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &["andromeda-layer-guard", "andromeda-ngsi-ld-lexicon"];

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
