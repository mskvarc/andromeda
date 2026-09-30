//! The UN/CEFACT unit-code catalog Andromeda stores attribute units against.
//!
//! An NGSI-LD `unitCode` is a UN/CEFACT Recommendation 20 common code, drawn
//! from a list of a few thousand tokens that is amended, never rewritten. That
//! shape is a dictionary the broker can hold entirely in a two-byte key, so a
//! unit is persisted as a [`UnitCodeId`](unit_code_id::UnitCodeId) rather than
//! as a repeated text token on every observation row.
//!
//! The identity is append-only and is not derived from anything the compiler
//! chooses. Rust enum discriminants and the source order of the generating
//! artifact both change when the artifact is regenerated, so neither may reach
//! a persisted row; the assignment lives in a committed snapshot instead, which
//! is what makes an identifier written by one release readable by the next.
//!
//! The catalog is a naming registry and nothing more. Recommendation 20 carries
//! informative conversion factors, and the broker deliberately does not act on
//! them: converting between units, and deciding what a query means when two
//! instances of one attribute carry different units, are semantic decisions the
//! NGSI-LD specification does not delegate to the storage layer.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// Resolving an expanded JSON-LD `unitCode` member to its catalog identity.
pub mod expanded_unit_code;
/// The revision of UN/CEFACT Recommendation 20 a registration belongs to.
pub mod rec20_revision;
/// The digest identifying the artifact a catalog release was generated from.
pub mod source_digest;
/// One registered unit code and the revision span it is valid for.
pub mod unit_code_entry;
/// The persisted two-byte identity of a unit code.
pub mod unit_code_id;
/// The append-only registry of unit-code identities.
pub mod unit_code_registry;
/// The alphanumeric common code token itself.
pub mod unit_code_token;

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}
