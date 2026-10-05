//! The typed, context-independent model of an NGSI-LD entity.
//!
//! ETSI GS CIM 009 v1.9.1 defines an entity (clause 4.5.1) as an identifier,
//! one or more types, optional scopes (clause 4.18) and an expiry (clause
//! 4.22), and attributes of eight families (clause 4.5), each attribute with at
//! most one instance per `datasetId` (clause 4.5.5). This crate holds that
//! state as [`entity_state::EntityState`], and what a write carries — which
//! may delete members with the NGSI-LD Null, omit members, or repeat a
//! `datasetId` — as [`document::EntityDocument`], a different type.
//!
//! The persisted and exchanged form is the standard expanded JSON-LD document
//! the broker has always stored. [`codec`] reads and writes it straight from
//! and to bytes, decoding a geometry's nested `@list`s directly into its
//! positions. [`admission`] holds the clause 5.5.4 rules a written document is
//! held to, [`reads`] the cheap reads the rest of the broker takes from a
//! state, and [`queried`] what the query languages read of a state or a
//! document.
//!
//! The model is keyed by expanded IRIs: a request's `@context` decides how
//! names are written on the wire, not what they are (clause 5.5.7), so neither
//! JSON-LD processing nor compaction lives here.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod admission;
pub mod attribute_instances;
pub mod attributes;
pub mod codec;
pub mod conversion;
pub mod date_time_text;
pub mod document;
pub mod entity_state;
pub mod entity_types;
pub mod expanded_value;
pub mod expanded_value_name;
pub mod family_value;
pub mod geo_value;
pub mod geometry_value;
pub mod history;
pub mod instance;
pub mod language_entries;
pub mod list_object;
pub mod literal;
pub mod member_role;
pub mod member_shape;
pub mod ngsi_ld_null;
pub mod node_id;
pub mod non_empty;
pub mod operation;
pub mod proof_seals;
pub mod queried;
pub mod reads;
pub mod refusal;
mod refusal_wording;
pub mod scope_refusal;
pub mod scope_slot;
pub mod scopes;
pub mod syntax_category;
pub mod system_instant;
pub mod system_time;
pub mod unit_code;
pub mod value_mode;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-json-literal",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-unit-catalog",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-test-allocator",
        "andromeda-test-fixtures",
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
