//! The extensions the broker installs when the server's installation ships
//! them, though no part of the schema depends on them.
//!
//! The migrations create only what the schema needs. An optional extension
//! serves an operator-facing feature that degrades without it, so it is
//! installed by the startup reconciliation that also updates the versioned
//! extensions: that reconciliation runs on every tenant database the broker
//! opens, so a server that gains the extension later reaches the databases
//! created before it as well as the ones created after. `pgstattuple` serves
//! the maintenance controller's index bloat measurement, and `pg_repack` the
//! online table repack the controller drives to give a bloated heap's free
//! space back to the filesystem.

/// An extension installed when the server ships it and the connecting role may
/// create it, and otherwise left absent.
///
/// Installing one grants nothing: the privileges a feature needs to use it
/// stay the operator's decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sea_query::Iden, strum::Display, strum::EnumIter, strum::IntoStaticStr)]
pub enum OptionalExtension {
    /// `pgstattuple`, the relation bloat diagnostics the maintenance
    /// controller measures B-tree and `GiST` indexes with; without it those
    /// indexes go unmeasured and every other maintenance step still runs.
    #[strum(serialize = "pgstattuple")]
    #[iden = "pgstattuple"]
    Pgstattuple,
    /// `pg_repack`, the online table rewrite the maintenance controller
    /// repacks a bloated heap through, giving its free space back to the
    /// filesystem while the table stays readable and writable; without it no
    /// table is repacked and every other maintenance step still runs.
    ///
    /// Untrusted, so only a superuser role installs it.
    #[strum(serialize = "pg_repack")]
    #[iden = "pg_repack"]
    PgRepack,
}

impl OptionalExtension {
    /// The extension's name in `pg_extension` and `pg_available_extensions`.
    #[must_use]
    pub fn catalog_name(self) -> &'static str {
        self.into()
    }

    /// The column alias a statement reads whether the extension is due to be
    /// installed under.
    #[must_use]
    pub const fn installable_column(self) -> &'static str {
        match self {
            OptionalExtension::Pgstattuple => "pgstattuple_installable",
            OptionalExtension::PgRepack => "pg_repack_installable",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::optional_extension::OptionalExtension;
    use andromeda_postgresql_schema::tenancy::EXTENSION_NAME_BYTE_LIMIT;
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    #[test]
    fn each_optional_extension_is_named_and_created_under_its_catalogue_name() {
        let names: Vec<&str> = OptionalExtension::iter().map(OptionalExtension::catalog_name).collect();
        assert_eq!(names, ["pgstattuple", "pg_repack"]);
        for extension in OptionalExtension::iter() {
            assert_eq!(sea_query::Iden::unquoted(&extension), extension.catalog_name());
            assert!(extension.catalog_name().len() <= EXTENSION_NAME_BYTE_LIMIT, "{extension}");
        }
    }

    #[test]
    fn the_installable_columns_of_every_optional_extension_are_distinct() {
        let mut columns: Vec<&str> = OptionalExtension::iter().map(OptionalExtension::installable_column).collect();
        columns.sort_unstable();
        columns.dedup();
        assert_eq!(columns.len(), 2);
    }
}
