//! A migration step whose text is a schema command built in code.
//!
//! The migrator applies and checksums migration text; a migration set
//! assembled in code (a step appended to the embedded set to prove an
//! upgrade) still needs that text, so it is rendered here from the command
//! that describes the change rather than written by hand.

use crate::command::Command;
use sqlx::{
    AssertSqlSafe,
    SqlSafeStr,
    migrate::{Migration, MigrationType},
};
use std::borrow::Cow;

/// The simple migration `version`, described as `description`, that applies
/// `command`.
#[must_use]
pub fn schema_migration(version: i64, description: &'static str, command: &impl Command) -> Migration {
    Migration::new(
        version,
        Cow::Borrowed(description),
        MigrationType::Simple,
        AssertSqlSafe(command.rendered()).into_sql_str(),
        false,
    )
}

#[cfg(test)]
mod tests {
    use crate::schema_migration::schema_migration;
    use sea_query::{Alias, ColumnDef, Table};

    #[test]
    fn the_migration_applies_the_rendered_command() {
        let alter = Table::alter()
            .table(Alias::new("entities"))
            .add_column(ColumnDef::new(Alias::new("appended_branch_probe")).integer())
            .to_owned();
        let migration = schema_migration(7, "appended branch column", &alter);
        pretty_assertions::assert_eq!(migration.version, 7);
        pretty_assertions::assert_eq!(migration.description, "appended branch column");
        pretty_assertions::assert_eq!(migration.sql.as_str(), r#"ALTER TABLE "entities" ADD COLUMN "appended_branch_probe" integer"#);
    }
}
