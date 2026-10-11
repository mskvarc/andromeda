//! Compacting the typed entity model into the compact representation a request
//! asked for (ETSI GS CIM 009 v1.9.1 clause 5.5.7, JSON-LD 1.1 API section 6).
//!
//! The model of `andromeda-ngsi-ld-entity` is keyed by expanded IRIs; a
//! response is keyed by the terms of the request's `@context`. Compacting an
//! NGSI-LD entity reduces to compacting IRIs -- attribute names, types,
//! `vocab` values, members of structured values -- and choosing the term
//! definition each value is written under: `@id` coercion, typed literals,
//! language maps, `@list` containers, `@json`. The JSON-LD library decides each
//! of those one term at a time against the processed request context, and the
//! walk here asks it for every key and value of the entity in turn, writing
//! the compacted document directly from the model instead of first building
//! the expanded document and handing it to the processor.
//!
//! The processor stays the authority: what the walk cannot decide one term at
//! a time -- scoped contexts, `@nest`, map containers -- is compacted by the
//! processor from the model's canonical expanded form, and the walk's output
//! is pinned to the processor's by differential tests over every attribute
//! family and the contexts that change term selection.
//!
//! [`entity_compactor::EntityCompactor`] compacts entity state into the compact
//! `Entity` the read path consumes, and written documents into compact JSON;
//! [`name_compaction::NameCompaction`] compacts a single expanded name, and is
//! the `TermCompactor` an expanded query filter is spelled back with.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod aggregated_entity;
mod carrier_items;
mod compacted;
mod compacted_mode;
mod compacted_node;
pub mod compaction_error;
mod core_aliases;
#[cfg(test)]
mod differential_tests;
mod document_compaction;
pub mod entity_compactor;
mod expanded_item;
mod geometry_compaction;
mod instance_compaction;
mod member_insertion;
pub mod name_compaction;
mod node_compaction;
mod patch_reading;
mod processor_fallback;
mod property_compaction;
mod reserved_terms;
mod state_compaction;
mod state_reading;
mod term_compaction;
mod term_context;
mod undecidable;
mod walk;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-json-literal",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-entity",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-vocabulary",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-test-allocator",
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
