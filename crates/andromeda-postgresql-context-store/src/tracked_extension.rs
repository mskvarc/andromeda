//! The extensions whose installed version the broker reads in every tenant
//! database and keeps in the tenant extension version record.
//!
//! An image upgrade can ship a newer version of each of them, and each is
//! brought to it only when the broker opens the database: a versioned
//! extension by its update scripts, a recreated one by dropping and creating it
//! again. The record of these versions is how the platform database finds the
//! tenants still behind without connecting to them. `pgstattuple` is not among
//! them: it ships with the server itself, so its version moves only with a
//! server upgrade.

use crate::{recreated_extension::RecreatedExtension, schema_contract::versioned_extension::VersionedExtension};
use strum::IntoEnumIterator;

/// An extension whose installed version the record tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackedExtension {
    /// A versioned extension the schema requires, brought up to date by
    /// `ALTER EXTENSION ... UPDATE`.
    Updated(VersionedExtension),
    /// An optional extension brought up to date by dropping and creating it
    /// again.
    Recreated(RecreatedExtension),
}

/// What brings an extension a database does not hold to the shipped version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbsentInstall {
    /// The migrations create it, so an open is never owed for its absence.
    CreatedByMigrations,
    /// The open installs it when the role may create it, so its absence is
    /// owed one open per shipped version.
    InstalledByOpen,
}

impl TrackedExtension {
    /// Every tracked extension: the versioned ones, then the recreated ones.
    pub fn all() -> impl Iterator<Item = TrackedExtension> {
        VersionedExtension::iter()
            .map(TrackedExtension::Updated)
            .chain(RecreatedExtension::iter().map(TrackedExtension::Recreated))
    }

    /// The extension's name in `pg_extension` and `pg_available_extensions`.
    #[must_use]
    pub fn catalog_name(self) -> &'static str {
        match self {
            TrackedExtension::Updated(extension) => extension.catalog_name(),
            TrackedExtension::Recreated(extension) => extension.catalog_name(),
        }
    }

    /// The column alias a statement reads the installed version under.
    #[must_use]
    pub const fn installed_column(self) -> &'static str {
        match self {
            TrackedExtension::Updated(extension) => extension.installed_column(),
            TrackedExtension::Recreated(extension) => extension.installed_column(),
        }
    }

    /// The column alias a statement reads the shipped version under.
    #[must_use]
    pub const fn shipped_column(self) -> &'static str {
        match self {
            TrackedExtension::Updated(extension) => extension.shipped_column(),
            TrackedExtension::Recreated(extension) => extension.shipped_column(),
        }
    }

    /// What brings the extension to the shipped version where it is absent.
    #[must_use]
    pub const fn absent_install(self) -> AbsentInstall {
        match self {
            TrackedExtension::Updated(_) => AbsentInstall::CreatedByMigrations,
            TrackedExtension::Recreated(_) => AbsentInstall::InstalledByOpen,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tracked_extension::{AbsentInstall, TrackedExtension};
    use andromeda_postgresql_schema::tenancy::EXTENSION_NAME_BYTE_LIMIT;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_record_tracks_postgis_timescaledb_and_pg_repack_in_that_order() {
        let names: Vec<&str> = TrackedExtension::all().map(TrackedExtension::catalog_name).collect();
        assert_eq!(names, ["postgis", "timescaledb", "pg_repack"]);
        for extension in TrackedExtension::all() {
            assert!(extension.catalog_name().len() <= EXTENSION_NAME_BYTE_LIMIT, "{extension:?}");
        }
    }

    #[test]
    fn the_installed_and_shipped_columns_of_every_tracked_extension_are_distinct() {
        let mut columns: Vec<&str> = TrackedExtension::all()
            .flat_map(|extension| [extension.installed_column(), extension.shipped_column()])
            .collect();
        columns.sort_unstable();
        columns.dedup();
        assert_eq!(columns.len(), 6);
    }

    #[test]
    fn only_a_recreated_extension_is_owed_an_open_for_its_absence() {
        let rules: Vec<(&str, AbsentInstall)> = TrackedExtension::all()
            .map(|extension| (extension.catalog_name(), extension.absent_install()))
            .collect();
        assert_eq!(
            rules,
            [
                ("postgis", AbsentInstall::CreatedByMigrations),
                ("timescaledb", AbsentInstall::CreatedByMigrations),
                ("pg_repack", AbsentInstall::InstalledByOpen),
            ]
        );
    }
}
