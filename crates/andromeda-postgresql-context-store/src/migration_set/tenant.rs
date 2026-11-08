//! The tenant migration set: `tenant/` together with `branch/`.
//!
//! The two directories share one version space and the tenant migrator runs
//! their union in version order in `public`: the global prerequisites
//! (extensions, functions, dictionaries), then the branch relations the live
//! branch holds, then the global relations that reference them, then the
//! TimescaleDB conversion of the live history tables.

use crate::migration_set::branch::branch_migrations;
use sqlx::migrate::{Migration, Migrator};
use std::{borrow::Cow, sync::LazyLock};

/// The ledger the tenant migration set is recorded in.
pub const TENANT_LEDGER: &str = "_andromeda_tenant_migrations";

/// The tenant-only migrations, exactly as embedded in the binary.
static TENANT_ONLY_MIGRATIONS: Migrator = sqlx::migrate!("../../migrations/tenant");

/// The union, built once.
static TENANT_MIGRATOR: LazyLock<Migrator> = LazyLock::new(|| {
    let mut migrator = Migrator {
        migrations: Cow::Owned(union(&TENANT_ONLY_MIGRATIONS.migrations, branch_migrations())),
        ..Migrator::DEFAULT
    };
    migrator.dangerous_set_table_name(TENANT_LEDGER);
    migrator
});

/// The migrator for a tenant database's `public` schema, recorded in
/// [`TENANT_LEDGER`].
#[must_use]
pub fn tenant_migrator() -> &'static Migrator {
    &TENANT_MIGRATOR
}

/// Two migration sets merged into one, sorted by version.
fn union(tenant: &[Migration], branch: &[Migration]) -> Vec<Migration> {
    // clone: the two embedded sets are separate static slices, so their union is an owned list.
    let mut migrations: Vec<Migration> = tenant.iter().chain(branch).cloned().collect();
    migrations.sort_by_key(|migration| migration.version);
    migrations
}

#[cfg(test)]
mod tests {
    use crate::migration_set::{
        branch::branch_migrations,
        platform::platform_migrations,
        tenant::{TENANT_LEDGER, TENANT_ONLY_MIGRATIONS, tenant_migrator},
    };
    use pretty_assertions::assert_eq;
    use std::{collections::HashSet, fs, path::PathBuf};

    /// The `<version>_<description>.sql` files of one migration directory.
    fn versions_on_disk(directory: &str) -> Vec<i64> {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../migrations").join(directory);
        let mut versions: Vec<i64> = fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("{} is readable: {error}", directory.display()))
            .map(|entry| entry.unwrap_or_else(|error| panic!("directory entry is readable: {error}")).file_name())
            .filter_map(|name| {
                let name = name.to_string_lossy().into_owned();
                name.strip_suffix(".sql")
                    .and_then(|stem| stem.split_once('_'))
                    .map(|(version, _)| version.parse::<i64>().unwrap_or_else(|error| panic!("{name} has a numeric version: {error}")))
            })
            .collect();
        versions.sort_unstable();
        versions
    }

    #[test]
    fn the_tenant_migrator_records_into_the_tenant_ledger() {
        assert_eq!(tenant_migrator().table_name, TENANT_LEDGER);
        assert_eq!(TENANT_LEDGER, "_andromeda_tenant_migrations");
        assert!(!tenant_migrator().ignore_missing);
    }

    /// The three directories share one version space; a version in two of
    /// them would make the union ambiguous and the numbering lie about order.
    #[test]
    fn no_version_is_used_by_more_than_one_migration_across_all_three_directories() {
        let all: Vec<i64> = platform_migrations()
            .iter()
            .chain(TENANT_ONLY_MIGRATIONS.iter())
            .chain(branch_migrations())
            .map(|migration| migration.version)
            .collect();
        let unique: HashSet<i64> = all.iter().copied().collect();
        assert_eq!(unique.len(), all.len(), "a version is reused: {all:?}");
    }

    /// One version space numbered without gaps, so a file's version is its
    /// position in the order every set applies.
    #[test]
    fn the_versions_of_all_three_directories_run_contiguously_from_one() {
        let mut all: Vec<i64> = platform_migrations()
            .iter()
            .chain(TENANT_ONLY_MIGRATIONS.iter())
            .chain(branch_migrations())
            .map(|migration| migration.version)
            .collect();
        all.sort_unstable();
        let all = all;
        let expected: Vec<i64> = (1..).take(all.len()).collect();
        assert_eq!(all, expected);
    }

    #[test]
    fn the_union_is_sorted_and_holds_exactly_the_files_on_disk() {
        let union: Vec<i64> = tenant_migrator().iter().map(|migration| migration.version).collect();
        let mut on_disk = versions_on_disk("tenant");
        on_disk.extend(versions_on_disk("branch"));
        on_disk.sort_unstable();
        assert_eq!(union, on_disk);
    }
}
