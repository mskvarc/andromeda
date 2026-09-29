//! The invariant that every stored entity carries at least one type.
//!
//! Clause 4.5.1 makes `type` a mandatory member of an Entity, and the
//! `entity_search` relation states the same thing as a check constraint. Stating it
//! twice is deliberate: the constraint is what makes a typeless row
//! unrepresentable no matter which statement wrote it, and the check below is
//! what lets the write path name the offending entity instead of aborting the
//! whole transaction on an anonymous constraint failure. Both report the same
//! typed error, so a caller never has to tell the two apart.

use crate::{entity::params::PreparedEntityColumns, error::ContextStoreError};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use thiserror::Error;

/// The name of the database check constraint that carries this invariant.
pub const ENTITY_TYPES_PRESENT_CONSTRAINT: &str = "chk_entity_search_entity_type_name_keys_present";

/// An entity write carried no type.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum EntityTypesMissing {
    /// The write path refused the row before the statement ran, so the entity
    /// it belongs to is known (ETSI GS CIM 009 clause 4.5.1).
    #[error("entity {0} carries no type, but at least one is required")]
    Rejected(Identifier),
    /// The database check constraint refused the row. The statement is set-based
    /// and the constraint names no row, so no identifier is available (ETSI GS CIM 009 clause 4.5.1).
    #[error("an entity write carried no type, but at least one is required")]
    ConstraintViolated,
}

/// Refuse a column set in which any entity carries no type.
///
/// A type column is an array of type dictionary keys; an empty one would land
/// in the table as an empty `integer[]`, which is the state the constraint
/// forbids.
///
/// # Errors
///
/// Returns [`EntityTypesMissing::Rejected`] naming the first entity whose type
/// column is empty.
pub fn types_present(columns: &PreparedEntityColumns) -> Result<(), EntityTypesMissing> {
    match columns.ids.iter().zip(&columns.entity_type_name_keys).find(|(_, types)| types.is_empty()) {
        Some((id, _)) => Err(EntityTypesMissing::Rejected(Identifier::clone(id))), // clone: the error outlives the borrowed column set.
        None => Ok(()),
    }
}

/// Lift a statement failure into the typed invariant when the entity types
/// check constraint is what refused the write.
#[must_use]
pub fn entity_write_failure(error: sqlx::Error) -> ContextStoreError {
    if let sqlx::Error::Database(database) = &error
        && database.constraint() == Some(ENTITY_TYPES_PRESENT_CONSTRAINT)
    {
        return EntityTypesMissing::ConstraintViolated.into();
    }
    ContextStoreError::from(error)
}

#[cfg(test)]
mod tests {
    use crate::entity::{
        params::PreparedEntityColumns,
        types_present::{EntityTypesMissing, types_present},
    };
    use andromeda_postgresql_encoding::array_literal::IntegerArrayLiteral;
    use pretty_assertions::assert_eq;

    /// Build a one-row column set carrying the given type keys.
    fn columns_with_types(types: &[i64]) -> PreparedEntityColumns {
        let mut columns = PreparedEntityColumns::with_capacity(1);
        columns.ids.push("urn:ngsi-ld:Building:1".parse().expect("entity URI"));
        columns.entity_type_name_keys.push(IntegerArrayLiteral::of(types.iter().copied()));
        columns
    }

    #[test]
    fn a_typed_entity_satisfies_the_invariant() {
        assert_eq!(types_present(&columns_with_types(&[7])), Ok(()));
    }

    #[test]
    fn an_entity_with_an_empty_type_array_is_named_in_the_rejection() {
        let expected = "urn:ngsi-ld:Building:1".parse().expect("entity URI");
        assert_eq!(types_present(&columns_with_types(&[])), Err(EntityTypesMissing::Rejected(expected)));
    }

    #[test]
    fn an_empty_column_set_satisfies_the_invariant() {
        assert_eq!(types_present(&PreparedEntityColumns::with_capacity(0)), Ok(()));
    }
}
