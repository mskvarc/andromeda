//! Transaction-scoped temporary copies of a relation's shape.
//!
//! sea-query has no form for `CREATE TEMPORARY TABLE ... (LIKE ...)` nor for
//! `ON COMMIT DROP`, so the command is spelled here, its names quoted through
//! the same idens every other statement names the relations by.

use crate::{
    command::{Command, rendering::RenderedCommand},
    identifier::{quoted_iden, quoted_table},
    pg_type::PostgresType,
};
use sea_query::{DynIden, Iden, IntoIden, TableName};

/// The schema PostgreSQL resolves a session's temporary relations in, which
/// a statement names to read a temporary relation rather than the persistent
/// one of the same name.
#[derive(Iden, Clone, Copy)]
#[iden = "pg_temp"]
pub struct TemporarySchema;

/// `CREATE TEMPORARY TABLE "<name>" (LIKE "<schema>"."<source>" INCLUDING
/// DEFAULTS INCLUDING INDEXES) ON COMMIT DROP`: an empty relation of the
/// columns, defaults and indexes of `source`, gone when the transaction that
/// created it ends.
///
/// The indexes come along so a statement written against `source` (an
/// `ON CONFLICT` naming its unique index included) runs unchanged against
/// the copy. Constraints, identities and triggers do not: the copy holds rows
/// the caller places, keys included.
#[derive(Clone, Debug, PartialEq)]
pub struct TemporaryCopy {
    /// The temporary relation created.
    name: DynIden,
    /// The relation whose shape it copies.
    source: TableName,
}

impl TemporaryCopy {
    /// A temporary relation `name` shaped like `source`.
    pub fn new(name: impl IntoIden, source: impl Into<TableName>) -> TemporaryCopy {
        TemporaryCopy {
            name: name.into_iden(),
            source: source.into(),
        }
    }
}

impl RenderedCommand for TemporaryCopy {
    fn rendered(&self) -> String {
        format!(
            "CREATE TEMPORARY TABLE {} (LIKE {} INCLUDING DEFAULTS INCLUDING INDEXES) ON COMMIT DROP",
            quoted_iden(&self.name),
            quoted_table(&self.source)
        )
    }
}

impl Command for TemporaryCopy {}

/// What a column of a [`TemporaryTable`] may hold.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColumnConstraint {
    /// The column may be NULL.
    Nullable,
    /// The column is `NOT NULL`.
    NotNull,
    /// The column is the `PRIMARY KEY`.
    PrimaryKey,
}

/// One column of a [`TemporaryTable`].
#[derive(Clone, Debug)]
pub struct TemporaryColumn {
    /// The column's name.
    name: DynIden,
    /// The column's type.
    ty: PostgresType,
    /// What the column may hold.
    constraint: ColumnConstraint,
}

impl TemporaryColumn {
    /// A column `name` of type `ty` under `constraint`.
    pub fn new(name: impl IntoIden, ty: PostgresType, constraint: ColumnConstraint) -> TemporaryColumn {
        TemporaryColumn {
            name: name.into_iden(),
            ty,
            constraint,
        }
    }
}

/// `CREATE TEMPORARY TABLE "<name>" ("<column>" <type> [NOT NULL | PRIMARY
/// KEY], ...) ON COMMIT DROP`: a relation of the given columns, in the given
/// order, gone when the transaction that created it ends.
#[derive(Clone, Debug)]
pub struct TemporaryTable {
    /// The temporary relation created.
    name: DynIden,
    /// Its columns, in declaration order.
    columns: Vec<TemporaryColumn>,
}

impl TemporaryTable {
    /// A temporary relation `name` of `columns`.
    pub fn new(name: impl IntoIden, columns: Vec<TemporaryColumn>) -> TemporaryTable {
        TemporaryTable {
            name: name.into_iden(),
            columns,
        }
    }
}

impl RenderedCommand for TemporaryTable {
    fn rendered(&self) -> String {
        let columns = self
            .columns
            .iter()
            .map(|column| {
                let constraint = match column.constraint {
                    ColumnConstraint::Nullable => "",
                    ColumnConstraint::NotNull => " NOT NULL",
                    ColumnConstraint::PrimaryKey => " PRIMARY KEY",
                };
                format!("{} {}{constraint}", quoted_iden(&column.name), Iden::to_string(&column.ty))
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("CREATE TEMPORARY TABLE {} ({columns}) ON COMMIT DROP", quoted_iden(&self.name))
    }
}

impl Command for TemporaryTable {}

#[cfg(test)]
mod tests {
    use crate::{
        command::rendering::RenderedCommand,
        pg_type::PostgresType,
        temporary_table::{ColumnConstraint, TemporaryColumn, TemporaryCopy, TemporarySchema, TemporaryTable},
    };
    use sea_query::{Alias, Iden};

    #[test]
    fn a_defined_table_lists_its_columns_in_order_with_their_constraints() {
        let table = TemporaryTable::new(
            Alias::new("fixture"),
            vec![
                TemporaryColumn::new(Alias::new("id"), PostgresType::Bigint, ColumnConstraint::PrimaryKey),
                TemporaryColumn::new(Alias::new("kind"), PostgresType::Smallint, ColumnConstraint::NotNull),
                TemporaryColumn::new(Alias::new("note"), PostgresType::Text, ColumnConstraint::Nullable),
            ],
        );
        pretty_assertions::assert_eq!(
            table.rendered(),
            r#"CREATE TEMPORARY TABLE "fixture" ("id" bigint PRIMARY KEY, "kind" smallint NOT NULL, "note" text) ON COMMIT DROP"#
        );
    }

    #[test]
    fn the_copy_is_temporary_and_names_its_source_qualified() {
        pretty_assertions::assert_eq!(
            TemporaryCopy::new(Alias::new("entities"), (Alias::new("public"), Alias::new("entities"))).rendered(),
            r#"CREATE TEMPORARY TABLE "entities" (LIKE "public"."entities" INCLUDING DEFAULTS INCLUDING INDEXES) ON COMMIT DROP"#
        );
        pretty_assertions::assert_eq!(TemporarySchema.to_string(), "pg_temp");
    }
}
