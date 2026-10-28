//! What a Snapshot's branch schema holds of the instance table.
//!
//! A Snapshot's footprint is its whole branch schema, so the instance table
//! and its key index count without being listed, and reaping drops the schema
//! with everything it holds. A clone copies the instance rows as they are:
//! they carry their own grain, so no surrogate key continues, and the
//! sub-attribute tree is a column of one tenant-wide type.

use crate::repository::{PinError, index, schema};
use andromeda_sql::{
    declared_schema::created_object::{CreatedObject, ObjectKind},
    migration_set::MigrationSet,
};
use pretty_assertions::assert_eq;

/// The relation holding every current attribute instance.
const CURRENT_ATTRIBUTES: &str = "current_attributes";

#[test]
fn the_instance_table_and_its_key_index_live_in_the_branch_schema() -> Result<(), PinError> {
    let schema = schema()?;
    let table = schema
        .tables()
        .iter()
        .find(|table| table.name() == CURRENT_ATTRIBUTES)
        .ok_or_else(|| PinError::Missing(CURRENT_ATTRIBUTES.to_owned()))?;
    assert_eq!(schema.set_of(table.origin()), Some(MigrationSet::Branch), "an ordinary table of every branch");
    let key_index = index(&schema, "idx_ca_sub_eq_keys")?;
    assert_eq!(
        (key_index.table(), schema.set_of(key_index.origin())),
        (CURRENT_ATTRIBUTES, Some(MigrationSet::Branch)),
        "the key index lives with its table in the branch schema"
    );
    Ok(())
}

#[test]
fn the_instance_table_has_no_key_to_continue_and_holds_its_sub_attribute_tree() -> Result<(), PinError> {
    let schema = schema()?;
    let declared: Vec<_> = schema.columns().iter().filter(|column| column.relation() == CURRENT_ATTRIBUTES).collect();
    assert!(declared.iter().all(|column| !column.is_identity()), "no key to continue");
    assert!(declared.iter().any(|column| column.name() == "sub_attributes"));
    Ok(())
}

/// `sub_attribute` is declared once per tenant in `public`, and both branches
/// resolve it there, so the source and target arrays are of one type: two
/// composite types of the same shape in two schemas would not cast to each
/// other, and the copy casts nothing.
#[test]
fn the_sub_attribute_tree_is_of_one_tenant_wide_type() -> Result<(), PinError> {
    let schema = schema()?;
    let sub_attributes = schema
        .columns()
        .iter()
        .find(|column| column.relation() == CURRENT_ATTRIBUTES && column.name() == "sub_attributes")
        .ok_or_else(|| PinError::Missing("current_attributes.sub_attributes".to_owned()))?;
    assert_eq!(
        (sub_attributes.data_type().name().schema(), sub_attributes.data_type().written()),
        (None, "sub_attribute[]".to_owned()),
        "an unqualified type the search path resolves"
    );
    let in_branch: Vec<&str> = schema
        .created()
        .iter()
        .filter(|object| object.kind() == ObjectKind::CompositeType && schema.set_of(object.origin()) == Some(MigrationSet::Branch))
        .map(CreatedObject::name)
        .collect();
    assert!(in_branch.is_empty(), "types declared inside the branch schema: {in_branch:?}");
    Ok(())
}
