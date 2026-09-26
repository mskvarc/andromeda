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
pub mod attribute_name_csv;
pub mod attribute_strip;
pub mod context_source_identity;
pub mod context_source_info;
pub mod entity;
pub mod entity_info;
pub mod entity_map;
pub mod entity_map_fingerprint;
pub mod entity_type;
pub mod entity_type_list;
pub mod expanded;
pub mod jsonld_context;
pub mod key_value_pair;
pub mod lifetime_expiry;
pub mod notification;
pub mod query;
pub mod registration;
pub mod registration_management;
pub mod registration_matching;
pub mod scope_instance_value;
pub mod snapshot;
pub mod snapshot_clone;
pub mod snapshot_filter;
pub mod snapshot_input;
pub mod snapshot_patch;
pub mod subscription;
pub mod temporal;
pub mod tenant;
pub mod time_interval;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-vocabulary",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
