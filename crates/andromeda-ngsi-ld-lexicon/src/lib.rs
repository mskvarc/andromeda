//! The lexical layer of NGSI-LD: the grammars every named value in the API must satisfy.
//!
//! ETSI GS CIM 009 v1.9.1 defines its resources on top of a small set of
//! lexical productions — the short-name grammar of clause 4.6.2, the scope
//! path of clause 4.18, the dotted attribute path of clause 4.5.3, the `Date`
//! and `Time` forms of clause 4.6.3, and the URI that names every resource.
//! This crate owns those productions and the newtypes built directly on them,
//! so that a value which has passed one of these parsers is known-valid
//! everywhere above.
//!
//! It sits below the data model rather than inside it: the query languages, the
//! HTTP binding and the stores all name these types without depending on the
//! resources of clause 5.2.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

#[macro_use]
mod validated_name;

pub mod aggr_method;
pub mod attribute_name;
pub mod attribute_path;
pub mod attribute_type;
pub mod constant_string;
pub mod date_time;
pub mod entity_type_name;
pub mod name;
pub mod ngsi_ld_urn;
pub mod reference;
pub mod rfc3339;
pub mod scope;
pub mod temporal_format;
pub mod tenant;
pub mod time_prop;
pub mod time_rel;
pub mod uri;

#[cfg(test)]
mod boundary {
    /// The layer rule this crate is on the inside of.
    ///
    /// Asserted here rather than by a central test so the failure names the
    /// crate that broke the rule, and so a crate added to the workspace cannot
    /// be forgotten by a table it was never added to. The rule itself is read
    /// once, in `andromeda-layer-guard`, which every crate carries as a
    /// dev-dependency and which declares nothing of its own.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// In-workspace crates this one may name, in any dependency table.
        /// Anything else is an upward or sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &["andromeda-layer-guard", "andromeda-ngsi-ld-vocabulary"];

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
