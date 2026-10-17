//! The extensions whose installed version the tenant schema depends on.

/// An extension the tenant schema requires at a minimum version, and which the
/// broker keeps at the version the server's installation ships.
///
/// The contrib extensions the migrations also create ship with the server
/// itself, so their version moves only with a server upgrade and is not
/// tracked here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumIter, strum::IntoStaticStr)]
pub enum VersionedExtension {
    /// `PostGIS`, named `postgis` in the extension catalogue.
    #[strum(serialize = "postgis")]
    PostGis,
    /// `TimescaleDB`, named `timescaledb` in the extension catalogue.
    #[strum(serialize = "timescaledb")]
    TimescaleDb,
}

impl VersionedExtension {
    /// The extension's name in `pg_extension` and `pg_available_extensions`.
    #[must_use]
    pub fn catalog_name(self) -> &'static str {
        self.into()
    }

    /// The column alias a statement reads the installed version under.
    #[must_use]
    pub const fn installed_column(self) -> &'static str {
        match self {
            VersionedExtension::PostGis => "postgis_version",
            VersionedExtension::TimescaleDb => "timescaledb_version",
        }
    }

    /// The column alias a statement reads the shipped version under.
    #[must_use]
    pub const fn shipped_column(self) -> &'static str {
        match self {
            VersionedExtension::PostGis => "postgis_shipped_version",
            VersionedExtension::TimescaleDb => "timescaledb_shipped_version",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::schema_contract::versioned_extension::VersionedExtension;
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    #[test]
    fn each_extension_is_named_as_the_extension_catalogue_names_it() {
        let names: Vec<&str> = VersionedExtension::iter().map(VersionedExtension::catalog_name).collect();
        assert_eq!(names, ["postgis", "timescaledb"]);
        assert_eq!(VersionedExtension::TimescaleDb.to_string(), "timescaledb");
    }

    #[test]
    fn the_installed_and_shipped_columns_of_every_extension_are_distinct() {
        let mut columns: Vec<&str> = VersionedExtension::iter()
            .flat_map(|extension| [extension.installed_column(), extension.shipped_column()])
            .collect();
        columns.sort_unstable();
        columns.dedup();
        assert_eq!(columns.len(), 4);
    }
}
