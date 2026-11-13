//! The branch migration set, which creates and migrates a Snapshot schema.
//!
//! A branch file runs in two kinds of place: in `public` as part of the tenant
//! set, building the live branch, and in every Snapshot branch schema,
//! building the branch when the Snapshot is created and migrating it as the
//! tenant's migrations advance. It therefore qualifies nothing with
//! `public.`, declares no foreign key to a relation outside the branch (a key
//! into a tenant-global dictionary would make every Snapshot's DDL lock that
//! dictionary against live writes; the live keys are a tenant-only
//! migration's), calls no TimescaleDB function (the live history tables are
//! hypertables, a Snapshot's are plain tables), and runs in a transaction,
//! since the branch migrator runs inside the Snapshot's creation transaction,
//! where a `-- no-transaction` migration could not. It may set a `TimescaleDB`
//! setting for its own transaction: a setting only bounds what `TimescaleDB`
//! does on a hypertable, and is inert beside a plain table.

use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_postgresql_branch::snapshot_schema_name;
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

/// The migrator for one Snapshot branch schema, recorded in that schema's own
/// `<schema>._andromeda_branch_migrations` ledger.
///
/// The ledger is qualified with the schema name the Snapshot's surrogate key
/// derives, never client input. The caller puts the schema first on the
/// search path before running the migrator, which is what makes the
/// unqualified branch DDL land in it.
#[must_use]
pub fn branch_migrator(key: SnapshotKey, branch: SnapshotBranch) -> Migrator {
    let mut migrator = Migrator {
        migrations: Cow::Borrowed(branch_migrations()),
        ..Migrator::DEFAULT
    };
    migrator.dangerous_set_table_name(branch_ledger(key, branch));
    migrator
}

/// The schema-qualified ledger of one Snapshot branch schema.
#[must_use]
pub fn branch_ledger(key: SnapshotKey, branch: SnapshotBranch) -> String {
    format!("{}.{BRANCH_LEDGER}", snapshot_schema_name(key, branch))
}

#[cfg(test)]
mod tests {
    use crate::migration_set::branch::{branch_ledger, branch_migrations, branch_migrator};
    use andromeda_context_space::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
    use pretty_assertions::assert_eq;

    #[test]
    fn the_branch_migrator_records_into_the_ledger_of_its_own_schema() {
        let key = SnapshotKey::try_from(7).expect("a positive key");
        let migrator = branch_migrator(key, SnapshotBranch::Published);
        assert_eq!(migrator.table_name, "snap_7._andromeda_branch_migrations");
        assert_eq!(branch_ledger(key, SnapshotBranch::Staging), "snap_7_stg._andromeda_branch_migrations");
        assert!(!migrator.ignore_missing);
        assert_eq!(migrator.iter().count(), branch_migrations().len());
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

    /// How a branch migration sets a `TimescaleDB` setting for its own
    /// transaction, which names `TimescaleDB` without calling it.
    const TRANSACTION_SETTING: &str = "set_config('timescaledb.";

    #[test]
    fn no_branch_migration_calls_timescaledb() {
        for migration in branch_migrations() {
            let sql = migration.sql.as_str().replace(TRANSACTION_SETTING, "");
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
}
