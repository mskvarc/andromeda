//! Expanding a compact NGSI-LD write request into the typed written document.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.7 makes every term of a request mean what
//! the request's `@context` (the user context, then the protected core
//! context) expands it to. This crate reads a request into
//! `andromeda_ngsi_ld_entity::document::EntityDocument` (or one written
//! attribute instance) by asking the processed context, through the JSON-LD
//! library's own IRI expansion, for every decision: member keys, `@id`- and
//! `@vocab`-coerced values, typed literals, language maps, `@list` and
//! `@json` terms, and the property-scoped context of the core `ngsildproof`
//! term. A `GeoProperty`'s geometry goes from the positions the transport read
//! it into straight into the model, and every number keeps its exact text.
//!
//! The JSON-LD processor stays the authority: whatever the typed walk does not
//! model it declines ([`decline::Decline`]), and the processor expands that
//! request from the same processed context, its typed output read through the
//! same assembly ([`processor_decoding`]). [`entity_expansion::EntityExpansion`]
//! is the two together; [`typed_expander::TypedExpander`] is the walk alone.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

mod assembly;
pub mod attribute_fragment;
mod compact_walk;
pub mod decline;
pub mod entity_expansion;
pub mod error;
mod expanded_tree;
pub mod normalization;
pub mod processor_decoding;
mod processor_request;
pub mod received_state;
mod term_scope;
pub mod typed_expander;
pub mod typed_expansion;
mod typed_walk;
pub mod written_member;

#[cfg(test)]
mod context_tests;
#[cfg(test)]
mod differential_tests;
#[cfg(test)]
mod empty_value_tests;
#[cfg(test)]
mod expansion_fixture;
#[cfg(test)]
mod family_tests;
#[cfg(test)]
mod geometry_members_tests;
#[cfg(test)]
mod nesting_stack_tests;
#[cfg(test)]
mod null_sentinel_tests;
#[cfg(test)]
mod reserved_name_tests;
#[cfg(test)]
mod structured_value_name_tests;
#[cfg(test)]
mod temporal_tests;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-json-literal",
        "andromeda-jsonld",
        "andromeda-native-stack",
        "andromeda-ngsi-ld-entity",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-unit-catalog",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-test-allocator",
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
