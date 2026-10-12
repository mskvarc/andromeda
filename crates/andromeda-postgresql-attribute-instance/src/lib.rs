//! The stored form of one NGSI-LD attribute instance, shared by current state
//! and history.
//!
//! An instance is stored as its members (its attribute name key, `datasetId`,
//! family, value in exactly one typed column or field, `valueType`,
//! `objectType`, `unitCode`, `observedAt`, `expiresAt` and seals) and its
//! sub-attributes (clause 4.5.2) as `sub_attribute` elements in pre-order. This
//! crate owns those members, the element and the composite it travels as, the
//! dictionary keys every name is held as, and the flattening and assembly of
//! one instance and its tree. Where an instance sits (a current-state row, a
//! history row, a queue record) and the statements that write and read it
//! belong to the stores that hold it.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// Assembling one stored instance and its sub-attribute tree.
pub mod assemble;
/// Flattening one instance and its sub-attribute tree.
pub mod flatten;
/// The dictionary keys stored instances hold names as.
pub mod name_keys;
/// The family value an instance's stored columns hold.
mod stored_family;
/// A JSON piece held as its codec wrote it.
pub mod stored_json;
/// The members of one stored instance and their reading back.
pub mod stored_members;
/// The value of one instance, by the column that holds it.
pub mod stored_value;
/// One sub-attribute element and the composite it travels as.
pub mod sub_attribute;

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-entity",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
        "andromeda-postgresql-codes",
        "andromeda-postgresql-encoding",
        "andromeda-postgresql-query",
        "andromeda-unit-catalog",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["sqlx"],
}
