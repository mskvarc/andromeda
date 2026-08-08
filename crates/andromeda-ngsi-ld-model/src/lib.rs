//! The Andromeda NGSI-LD domain model.
//!
//! This crate holds the resources ETSI GS CIM 009 v1.9.1 defines — entities,
//! attributes, subscriptions, registrations and the discovery resources — built
//! on the lexical types of `andromeda-ngsi-ld-lexicon` and the IRI constants of
//! `andromeda-ngsi-ld-vocabulary`. It knows nothing about transport,
//! persistence, or the broker runtime, and every layer above it depends on
//! these types rather than on each other's.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod attribute;
pub mod attribute_list;
pub mod context_source_identity;
pub mod entity;
pub mod entity_map;
pub mod entity_type;
pub mod entity_type_list;
pub mod expanded;
pub mod jsonld_context;
pub mod key_value_pair;
pub mod notification;
pub mod query;
pub mod registration;
pub mod snapshot;
pub mod snapshot_filter;
pub mod subscription;
pub mod tenant_list;

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
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-layer-guard",
            "andromeda-ngsi-ld-geometry",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-query",
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
