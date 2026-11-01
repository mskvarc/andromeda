//! Every relation a logical replica can follow row by row.
//!
//! Logical replication identifies an updated or deleted row by the table's
//! replica identity: its primary key, or every column where the table sets
//! `replica identity full`. A table with neither publishes its inserts alone.
//! A hypertable's rows live in its chunks, which the parent's identity does
//! not reach.

use crate::repository::{PinError, schema};
use andromeda_sql::declared_schema::{
    constraint::ConstraintKind,
    table::{DeclaredTable, Hypertable, ReplicaIdentity, ReplicaIdentityKind},
};
use pretty_assertions::assert_eq;

#[test]
fn every_table_but_the_history_hypertable_has_a_primary_key_or_a_full_replica_identity() -> Result<(), PinError> {
    let schema = schema()?;
    let hypertables: Vec<&str> = schema.hypertables().iter().map(Hypertable::table).collect();
    assert_eq!(hypertables, ["attribute_history"]);
    let unidentified: Vec<&str> = schema
        .tables()
        .iter()
        .map(DeclaredTable::name)
        .filter(|table| !hypertables.contains(table))
        .filter(|table| {
            !schema
                .constraints()
                .iter()
                .any(|constraint| constraint.table() == *table && constraint.kind() == ConstraintKind::PrimaryKey)
        })
        .filter(|table| {
            !schema
                .replica_identities()
                .iter()
                .any(|identity| identity.table() == *table && identity.kind() == ReplicaIdentityKind::Full)
        })
        .collect();
    assert_eq!(unidentified, Vec::<&str>::new());
    Ok(())
}

#[test]
fn the_census_ledgers_are_identified_by_every_column() -> Result<(), PinError> {
    let schema = schema()?;
    let full: Vec<&str> = schema
        .replica_identities()
        .iter()
        .filter(|identity| identity.kind() == ReplicaIdentityKind::Full)
        .map(ReplicaIdentity::table)
        .collect();
    assert_eq!(
        full,
        ["entity_type_census", "entity_type_attribute_census", "attribute_census", "stream_type_census"]
    );
    Ok(())
}
