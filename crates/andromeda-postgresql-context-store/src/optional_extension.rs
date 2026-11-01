//! The extensions the broker installs when the server's installation ships
//! them, though no part of the schema depends on them.
//!
//! The migrations create only what the schema needs. An optional extension
//! serves an operator-facing feature that degrades without it, so it is
//! installed by the startup reconciliation that also updates the versioned
//! extensions: that reconciliation runs on every tenant database the broker
//! opens, so a server that gains the extension later reaches the databases
//! created before it as well as the ones created after.

/// An extension installed when the server ships it and the connecting role may
/// create it, and otherwise left absent.
///
/// Installing one grants nothing: the privileges a feature needs to use it
/// stay the operator's decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sea_query::Iden, strum::Display, strum::EnumIter, strum::IntoStaticStr)]
pub enum OptionalExtension {
    /// `pgstattuple`, the relation bloat diagnostics the opt-in maintenance
    /// controller measures with; without it the controller reports a
    /// capability deferral for the database.
    #[strum(serialize = "pgstattuple")]
    #[iden = "pgstattuple"]
    Pgstattuple,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::optional_extension::OptionalExtension;
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    #[test]
    fn each_optional_extension_is_named_and_created_under_its_catalogue_name() {
        let names: Vec<&str> = OptionalExtension::iter().map(OptionalExtension::catalog_name).collect();
        assert_eq!(names, ["pgstattuple"]);
        for extension in OptionalExtension::iter() {
            assert_eq!(sea_query::Iden::unquoted(&extension), extension.catalog_name());
        }
    }
}
