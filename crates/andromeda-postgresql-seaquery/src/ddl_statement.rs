//! sea-query's schema statements (`CREATE TABLE`, `DROP TABLE`, `ALTER TABLE`,
//! `CREATE INDEX`, ...) run as utility commands.
//!
//! A schema statement carries no bound value: PostgreSQL reads a column
//! default or a `CHECK` expression from the statement text, which sea-query
//! renders inline. It runs on the simple protocol like every other utility
//! command of this crate.

use crate::command::{Command, rendering::RenderedCommand};
use sea_query::{
    IndexCreateStatement,
    IndexDropStatement,
    PostgresQueryBuilder,
    TableAlterStatement,
    TableCreateStatement,
    TableDropStatement,
    TableRenameStatement,
    TableTruncateStatement,
};

/// Run each listed sea-query schema statement as a [`Command`].
macro_rules! schema_command {
    ($($statement:ty),+ $(,)?) => {
        $(
            impl RenderedCommand for $statement {
                fn rendered(&self) -> String {
                    self.to_string(PostgresQueryBuilder)
                }
            }

            impl Command for $statement {}
        )+
    };
}

schema_command!(
    IndexCreateStatement,
    IndexDropStatement,
    TableAlterStatement,
    TableCreateStatement,
    TableDropStatement,
    TableRenameStatement,
    TableTruncateStatement,
);

#[cfg(test)]
mod tests {
    use crate::command::rendering::RenderedCommand;
    use sea_query::{Alias, ColumnDef, Table};

    #[test]
    fn a_dropped_table_cascades_when_asked_to() {
        let drop = Table::drop().table(Alias::new("snap_7_stg_entities")).cascade().if_exists().to_owned();
        pretty_assertions::assert_eq!(drop.rendered(), r#"DROP TABLE IF EXISTS "snap_7_stg_entities" CASCADE"#);
    }

    #[test]
    fn a_created_table_renders_its_columns() {
        let create = Table::create()
            .table(Alias::new("layout_fixture"))
            .col(ColumnDef::new(Alias::new("id")).big_integer().not_null())
            .to_owned();
        pretty_assertions::assert_eq!(create.rendered(), "CREATE TABLE \"layout_fixture\" ( \"id\" bigint NOT NULL )");
    }
}
