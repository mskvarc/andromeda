//! The branch migration set, which creates and upgrades a Snapshot schema.
//!
//! A branch file is run twice over its lifetime in two different places: once
//! in `public` as part of the tenant set, creating the live branch, and once
//! per Snapshot schema, creating or upgrading that branch. It therefore
//! qualifies nothing with `public.`, declares no foreign key to a relation
//! outside the branch (a key into a tenant-global dictionary would make every
//! Snapshot's DDL lock that dictionary against live writes; the live keys are
//! a tenant-only migration's), calls no TimescaleDB function (the live
//! history tables are hypertables, a Snapshot's are plain tables), and runs in
//! a transaction, since the branch migrator runs inside the Snapshot's creation
//! transaction, where a `-- no-transaction` migration could not.

use andromeda_postgresql_schema::branch_schema_version::{BranchSchemaVersion, BranchVersionError};
use sqlx::migrate::{Migration, Migrator};
use std::borrow::Cow;

/// The name of the ledger each Snapshot schema records its branch migrations in.
pub const BRANCH_LEDGER: &str = "_andromeda_branch_migrations";

/// The branch migrations, exactly as embedded in the binary.
static BRANCH_MIGRATIONS: Migrator = sqlx::migrate!("../../migrations/branch");

/// The branch migrations, in version order.
#[must_use]
pub fn branch_migrations() -> &'static [Migration] {
    &BRANCH_MIGRATIONS.migrations[..]
}

/// A branch migration set a Snapshot schema is created or upgraded with.
///
/// The broker only ever runs [`BranchMigrationSet::embedded`]. A set built from
/// explicit migrations is how a release's upgrade path is rehearsed against
/// existing Snapshot schemas: the embedded set plus the migrations a later
/// release would append.
#[derive(Debug, Clone)]
pub struct BranchMigrationSet {
    /// The migrations, in version order.
    migrations: Cow<'static, [Migration]>,
}

impl BranchMigrationSet {
    /// The branch migrations this binary embeds.
    #[must_use]
    pub fn embedded() -> BranchMigrationSet {
        BranchMigrationSet {
            migrations: Cow::Borrowed(branch_migrations()),
        }
    }

    /// A set of explicit migrations, sorted by version.
    #[must_use]
    pub fn new(migrations: Vec<Migration>) -> BranchMigrationSet {
        let mut migrations = migrations;
        migrations.sort_by_key(|migration| migration.version);
        BranchMigrationSet {
            migrations: Cow::Owned(migrations),
        }
    }

    /// The migrations of this set, in version order.
    #[must_use]
    pub fn migrations(&self) -> &[Migration] {
        &self.migrations
    }

    /// The migrator for one Snapshot schema, recorded in that schema's own
    /// `<schema>._andromeda_branch_migrations` ledger.
    ///
    /// `schema` is a Snapshot schema name derived from a positive surrogate key
    /// (`andromeda_postgresql_branch::snapshot_schema_name`), never client
    /// input, so qualifying the ledger with it is safe. The caller puts the
    /// schema first on the search path before running the migrator, which is
    /// what makes the unqualified branch DDL land in it.
    #[must_use]
    pub fn migrator(&self, schema: &str) -> Migrator {
        // clone: the migrator needs its own list. For the embedded set this copies
        // only the slice reference.
        let mut migrator = Migrator {
            migrations: self.migrations.clone(),
            ..Migrator::DEFAULT
        };
        migrator.dangerous_set_table_name(branch_ledger(schema));
        migrator
    }

    /// The highest version of this set, which a Snapshot's schemas carry once
    /// they have been brought to it.
    ///
    /// # Errors
    ///
    /// Returns [`BranchVersionError`] when the set is empty or its highest
    /// version does not fit the stored `INTEGER` column.
    pub fn version(&self) -> Result<BranchSchemaVersion, BranchVersionError> {
        self.migrations
            .iter()
            .filter(|migration| !migration.migration_type.is_down_migration())
            .map(|migration| migration.version)
            .max()
            .ok_or(BranchVersionError::NoMigrations)
            .and_then(BranchSchemaVersion::try_from)
    }
}

/// The schema-qualified ledger of one Snapshot schema.
#[must_use]
pub fn branch_ledger(schema: &str) -> String {
    format!("{schema}.{BRANCH_LEDGER}")
}

/// The highest branch migration version this binary embeds, which a Snapshot's
/// schemas must carry to be served.
///
/// # Errors
///
/// Returns [`BranchVersionError`] when the embedded branch set is empty or its
/// highest version does not fit the stored `INTEGER` column.
pub fn current_branch_version() -> Result<BranchSchemaVersion, BranchVersionError> {
    BranchMigrationSet::embedded().version()
}

#[cfg(test)]
mod tests {
    use crate::migration_set::branch::{BranchMigrationSet, branch_ledger, branch_migrations, current_branch_version};
    use pretty_assertions::assert_eq;

    #[test]
    fn the_branch_migrator_records_into_the_ledger_of_its_own_schema() {
        let migrator = BranchMigrationSet::embedded().migrator("snap_7");
        assert_eq!(migrator.table_name, "snap_7._andromeda_branch_migrations");
        assert_eq!(branch_ledger("snap_7_stg"), "snap_7_stg._andromeda_branch_migrations");
        assert!(!migrator.ignore_missing);
    }

    /// A branch file also runs inside a Snapshot schema, so it must let the
    /// search path decide where its relations land and must be valid against
    /// the plain tables a Snapshot holds.
    #[test]
    fn no_branch_migration_qualifies_a_relation_with_the_public_schema() {
        for migration in branch_migrations() {
            assert!(
                !migration.sql.as_str().contains("public."),
                "branch migration {} ({}) names `public.`",
                migration.version,
                migration.description
            );
        }
    }

    /// The whitespace- and parenthesis-separated tokens of every branch
    /// migration, `--` comments removed.
    fn branch_sql_tokens() -> Vec<&'static str> {
        branch_migrations()
            .iter()
            .flat_map(|migration| migration.sql.as_str().lines())
            .map(|line| line.split_once("--").map_or(line, |(code, _)| code))
            .flat_map(|code| code.split(|character: char| character.is_whitespace() || matches!(character, '(' | ')' | ',' | ';')))
            .filter(|token| !token.is_empty())
            .collect()
    }

    /// A foreign key locks the relation it references whenever its own table
    /// is created or dropped, and a Snapshot schema is created and dropped
    /// while live writes run. A key from a branch relation into a
    /// tenant-global one would make that DDL lock the shared relation against
    /// every live write, so a branch file may reference only relations the
    /// branch set itself creates.
    #[test]
    fn no_branch_migration_references_a_relation_outside_the_branch() {
        let tokens = branch_sql_tokens();
        let created: Vec<&str> = tokens
            .windows(2)
            .filter(|pair| pair[0].eq_ignore_ascii_case("TABLE"))
            .map(|pair| pair[1])
            .collect();
        let referenced: Vec<&str> = tokens
            .windows(2)
            .filter(|pair| pair[0].eq_ignore_ascii_case("REFERENCES"))
            .map(|pair| pair[1])
            .collect();
        assert!(created.contains(&"entities"), "the token scan finds the branch relations: {created:?}");
        assert!(!referenced.is_empty(), "the token scan finds the branch foreign keys");
        for relation in referenced {
            assert!(
                created.contains(&relation),
                "a branch migration references `{relation}`, which the branch set does not create"
            );
        }
    }

    #[test]
    fn no_branch_migration_calls_timescaledb() {
        for migration in branch_migrations() {
            let sql = migration.sql.as_str();
            for forbidden in ["create_hypertable", "timescaledb", "convert_to_columnstore"] {
                assert!(
                    !sql.contains(forbidden),
                    "branch migration {} ({}) names `{forbidden}`, which a Snapshot's plain tables cannot take",
                    migration.version,
                    migration.description
                );
            }
        }
    }

    /// The branch migrator runs inside the Snapshot's creation transaction.
    #[test]
    fn every_branch_migration_runs_in_a_transaction() {
        for migration in branch_migrations() {
            assert!(
                !migration.no_tx,
                "branch migration {} ({}) opts out of the transaction",
                migration.version, migration.description
            );
        }
    }

    #[test]
    fn the_current_branch_version_is_the_highest_embedded_branch_migration() {
        let highest = branch_migrations().iter().map(|migration| migration.version).max();
        assert_eq!(current_branch_version().map(|version| i64::from(version.get())).ok(), highest);
    }
}
