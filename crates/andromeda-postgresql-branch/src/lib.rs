//! The PostgreSQL branch schema of a context target.
//!
//! A context branch is a PostgreSQL schema: the live branch is the default
//! schema and a Snapshot branch is a dedicated one named after the Snapshot's
//! surrogate key. This crate is the one place the domain branch selector
//! ([`ContextTarget`]) meets its physical schema, so the naming convention
//! `snap_<key>` / `snap_<key>_stg` is stated exactly once. Statement builders
//! qualify their table references through [`BranchSchema`]; schema DDL reads the
//! plain name through [`snapshot_schema_name`].

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

use andromeda_context_space::{context_target::ContextTarget, snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_postgresql_seaquery::branch::qualified;
use sea_query::{Alias, DynIden, IntoIden, TableRef};

/// The PostgreSQL schema name of a Snapshot branch.
///
/// The published branch is `snap_<key>` and the private staging branch is
/// `snap_<key>_stg`, both derived from the Snapshot surrogate key so no branch
/// name has to be stored. `<key>` is a positive integer, so the result is
/// always a valid unquoted PostgreSQL identifier well within the 63-byte limit.
#[must_use]
pub fn snapshot_schema_name(key: SnapshotKey, branch: SnapshotBranch) -> String {
    match branch {
        SnapshotBranch::Published => format!("snap_{}", key.as_i64()),
        SnapshotBranch::Staging => format!("snap_{}_stg", key.as_i64()),
    }
}

/// The branch schema a context target's statements are qualified with.
///
/// Constructed once per statement and reused for every table it names: the live
/// branch carries no schema and a Snapshot branch carries its schema
/// identifier, cloned (a cheap `SeaRc` bump) into each qualified reference.
#[derive(Debug, Clone)]
pub struct BranchSchema(Option<DynIden>);

impl BranchSchema {
    /// Resolve the branch schema of a context target.
    #[must_use]
    pub fn of(target: &ContextTarget) -> BranchSchema {
        BranchSchema(
            target
                .snapshot()
                .map(|snapshot| Alias::new(snapshot_schema_name(snapshot.key(), snapshot.branch())).into_iden()),
        )
    }

    /// The branch schema identifier, or `None` for the live branch.
    #[must_use]
    pub const fn iden(&self) -> Option<&DynIden> {
        self.0.as_ref()
    }

    /// Qualify `table` with this branch schema.
    #[must_use]
    pub fn table(&self, table: impl IntoIden) -> TableRef {
        qualified(self.0.as_ref(), table)
    }
}

/// Qualify `table` with a context target's branch schema in one call.
///
/// Prefer [`BranchSchema::of`] when a statement names more than one table, so
/// the schema identifier is resolved once rather than per table.
#[must_use]
pub fn table_of(target: &ContextTarget, table: impl IntoIden) -> TableRef {
    BranchSchema::of(target).table(table)
}

/// The branch schema identifier of a context target, or `None` for the live
/// branch, for a statement builder that threads the schema into a row-reference
/// helper of its own.
#[must_use]
pub fn schema_iden(target: &ContextTarget) -> Option<DynIden> {
    BranchSchema::of(target).0
}

/// A dotted schema prefix for a hand-written SQL statement that cannot go
/// through sea-query (a binary `COPY` materialisation, say): `""` for the live
/// branch's default schema, `"snap_<key>."` for a Snapshot branch, so
/// `format!("{}entities", schema_prefix(target))` names the branch's table.
#[must_use]
pub fn schema_prefix(target: &ContextTarget) -> String {
    target
        .snapshot()
        .map_or_else(String::new, |snapshot| format!("{}.", snapshot_schema_name(snapshot.key(), snapshot.branch())))
}

#[cfg(test)]
mod tests {
    use crate::{BranchSchema, snapshot_schema_name, table_of};
    use andromeda_context_space::{context_target::ContextTarget, snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use sea_query::{Asterisk, PostgresQueryBuilder, Query};

    fn snapshot_target(branch: SnapshotBranch) -> ContextTarget {
        let key = SnapshotKey::try_from(7).expect("positive key");
        let id = "urn:ngsi-ld:Snapshot:abc".parse().expect("valid snapshot uri");
        ContextTarget::Snapshot(match branch {
            SnapshotBranch::Published => SnapshotTarget::published(key, id),
            SnapshotBranch::Staging => SnapshotTarget::staging(key, id),
        })
    }

    #[test]
    fn published_and_staging_schema_names_derive_from_the_key() {
        let key = SnapshotKey::try_from(7).expect("positive key");
        pretty_assertions::assert_eq!(snapshot_schema_name(key, SnapshotBranch::Published), "snap_7");
        pretty_assertions::assert_eq!(snapshot_schema_name(key, SnapshotBranch::Staging), "snap_7_stg");
    }

    #[test]
    fn the_live_branch_resolves_to_no_schema() {
        assert!(BranchSchema::of(&ContextTarget::Live).iden().is_none());
    }

    fn render(target: &ContextTarget) -> String {
        Query::select()
            .column(Asterisk)
            .from(table_of(target, sea_query::Alias::new("entities")))
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    #[test]
    fn the_live_branch_names_the_table_unqualified() {
        assert!(render(&ContextTarget::Live).contains(r#"FROM "entities""#));
    }

    #[test]
    fn a_snapshot_published_branch_qualifies_the_table() {
        assert!(render(&snapshot_target(SnapshotBranch::Published)).contains(r#"FROM "snap_7"."entities""#));
    }

    #[test]
    fn a_snapshot_staging_branch_qualifies_the_table() {
        assert!(render(&snapshot_target(SnapshotBranch::Staging)).contains(r#"FROM "snap_7_stg"."entities""#));
    }
}
