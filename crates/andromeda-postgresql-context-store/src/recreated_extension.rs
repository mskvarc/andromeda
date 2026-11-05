//! The optional extensions that ship no update scripts, so only dropping and
//! creating them again reaches the version the server ships.
//!
//! `ALTER EXTENSION ... UPDATE` reaches a newer version only through the
//! update scripts an extension ships. An extension that ships none stays at the
//! version it was created at under every later image, so the broker drops it
//! and creates it again instead. That is sound only for an extension that keeps
//! no state of its own across the drop.

use crate::optional_extension::OptionalExtension;

/// An optional extension brought to the version the server ships by dropping
/// it and creating it again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumIter)]
pub enum RecreatedExtension {
    /// `pg_repack`, whose objects outside a run in progress are its own
    /// functions and the `repack` schema, all created again with it.
    PgRepack,
}

impl RecreatedExtension {
    /// The optional extension this is, which names it in every command.
    #[must_use]
    pub const fn optional(self) -> OptionalExtension {
        match self {
            RecreatedExtension::PgRepack => OptionalExtension::PgRepack,
        }
    }

    /// The extension's name in `pg_extension` and `pg_available_extensions`.
    #[must_use]
    pub fn catalog_name(self) -> &'static str {
        self.optional().catalog_name()
    }

    /// The column alias a statement reads the installed version under.
    #[must_use]
    pub const fn installed_column(self) -> &'static str {
        match self {
            RecreatedExtension::PgRepack => "pg_repack_version",
        }
    }

    /// The column alias a statement reads the shipped version under.
    #[must_use]
    pub const fn shipped_column(self) -> &'static str {
        match self {
            RecreatedExtension::PgRepack => "pg_repack_shipped_version",
        }
    }

    /// The column alias a statement reads whether the session's role may
    /// create the extension under.
    #[must_use]
    pub const fn creatable_column(self) -> &'static str {
        match self {
            RecreatedExtension::PgRepack => "pg_repack_creatable",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{optional_extension::OptionalExtension, recreated_extension::RecreatedExtension};
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    #[test]
    fn pg_repack_is_recreated_under_its_optional_extension_name() {
        assert_eq!(RecreatedExtension::PgRepack.optional(), OptionalExtension::PgRepack);
        assert_eq!(RecreatedExtension::PgRepack.catalog_name(), "pg_repack");
    }

    #[test]
    fn the_columns_of_every_recreated_extension_are_distinct() {
        let mut columns: Vec<&str> = RecreatedExtension::iter()
            .flat_map(|extension| [extension.installed_column(), extension.shipped_column(), extension.creatable_column()])
            .collect();
        columns.sort_unstable();
        columns.dedup();
        assert_eq!(columns.len(), 3);
    }
}
