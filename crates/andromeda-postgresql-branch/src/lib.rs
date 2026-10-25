//! The PostgreSQL branch schema of a context target.
//!
//! A context branch is a PostgreSQL schema: the live branch is the default
//! schema and a Snapshot branch is a dedicated one named after the Snapshot's
//! surrogate key. This crate is the one place the domain branch selector
//! ([`ContextTarget`]) meets its physical schema, so the naming convention
//! `snap_<key>` / `snap_<key>_stg` is stated exactly once. Statement builders
//! qualify their table references through [`BranchSchema`]; schema commands name
//! the schema through [`snapshot_schema`].

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

/// The PostgreSQL schema of a Snapshot branch, named as [`snapshot_schema_name`]
/// names it.
#[must_use]
pub fn snapshot_schema(key: SnapshotKey, branch: SnapshotBranch) -> DynIden {
    Alias::new(snapshot_schema_name(key, branch)).into_iden()
}

/// A POSIX regular expression matching exactly the published Snapshot branch
/// schema names [`snapshot_schema_name`] renders, for a catalog statement that
/// selects them: a staging schema never matches.
pub const PUBLISHED_SNAPSHOT_SCHEMA_PATTERN: &str = "^snap_[1-9][0-9]*$";

/// A POSIX regular expression matching every Snapshot branch schema name
/// [`snapshot_schema_name`] renders, published and staging alike. Its first
/// parenthesised subexpression is the Snapshot key, so `substring(name,
/// pattern)` extracts it.
pub const SNAPSHOT_SCHEMA_PATTERN: &str = "^snap_([1-9][0-9]*)(_stg)?$";

/// The Snapshot whose published branch `schema` is, or `None` for any other
/// schema, a staging branch included.
///
/// A name is accepted only when [`snapshot_schema_name`] renders it back
/// unchanged, so `snap_07` and `snap_0` are not branches.
#[must_use]
pub fn published_snapshot_key(schema: &str) -> Option<SnapshotKey> {
    let key = SnapshotKey::try_from(schema.strip_prefix("snap_")?.parse::<i64>().ok()?).ok()?;
    (snapshot_schema_name(key, SnapshotBranch::Published) == schema).then_some(key)
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
        BranchSchema(target.snapshot().map(|snapshot| snapshot_schema(snapshot.key(), snapshot.branch())))
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

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{BranchSchema, PUBLISHED_SNAPSHOT_SCHEMA_PATTERN, SNAPSHOT_SCHEMA_PATTERN, published_snapshot_key, snapshot_schema_name, table_of};
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
    fn a_published_schema_name_reads_back_as_its_key() {
        let key = SnapshotKey::try_from(42).expect("positive key");
        pretty_assertions::assert_eq!(published_snapshot_key(&snapshot_schema_name(key, SnapshotBranch::Published)), Some(key));
    }

    #[test]
    fn staging_and_foreign_schema_names_are_not_published_branches() {
        for schema in ["snap_7_stg", "snap_07", "snap_0", "snap_-7", "snap_", "public", "snapshot_7"] {
            pretty_assertions::assert_eq!(published_snapshot_key(schema), None, "{schema}");
        }
    }

    /// Both patterns name the key alike: dropping the staging suffix and the
    /// key capture from the branch pattern leaves the published one.
    #[test]
    fn the_branch_pattern_is_the_published_pattern_with_an_optional_staging_suffix() {
        pretty_assertions::assert_eq!(
            SNAPSHOT_SCHEMA_PATTERN.replace("(_stg)?", "").replace(['(', ')'], ""),
            PUBLISHED_SNAPSHOT_SCHEMA_PATTERN
        );
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
