//! A `GIN` index over one column, optionally under an operator class.
//!
//! sea-query names an index access method but not the operator class of a
//! column, which decides the operators the index serves.

use crate::{
    command::{Command, rendering::RenderedCommand},
    identifier::{quoted_iden, quoted_table},
};
use sea_query::{DynIden, IntoIden, TableName};

/// The operator class a `GIN` index over a `jsonb` column is built with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JsonbOperatorClass {
    /// The default class: containment and the key-existence operators.
    Default,
    /// `jsonb_path_ops`: containment only, in a smaller index.
    PathOps,
}

/// `CREATE INDEX <name> ON <table> USING GIN (<column> [jsonb_path_ops])`.
#[derive(Clone, Debug)]
pub struct CreateGinIndex {
    /// The index's name.
    name: DynIden,
    /// The table indexed.
    table: TableName,
    /// The `jsonb` column indexed.
    column: DynIden,
    /// The operator class the column is indexed under.
    operator_class: JsonbOperatorClass,
}

impl CreateGinIndex {
    /// A `GIN` index `name` over `column` of `table` under `operator_class`.
    pub fn new(name: impl IntoIden, table: impl Into<TableName>, column: impl IntoIden, operator_class: JsonbOperatorClass) -> CreateGinIndex {
        CreateGinIndex {
            name: name.into_iden(),
            table: table.into(),
            column: column.into_iden(),
            operator_class,
        }
    }
}

impl RenderedCommand for CreateGinIndex {
    fn rendered(&self) -> String {
        let operator_class = match self.operator_class {
            JsonbOperatorClass::Default => "",
            JsonbOperatorClass::PathOps => " jsonb_path_ops",
        };
        format!(
            "CREATE INDEX {} ON {} USING GIN ({}{operator_class})",
            quoted_iden(&self.name),
            quoted_table(&self.table),
            quoted_iden(&self.column)
        )
    }
}

impl Command for CreateGinIndex {}

#[cfg(test)]
mod tests {
    use crate::{
        command::rendering::RenderedCommand,
        gin_index::{CreateGinIndex, JsonbOperatorClass},
    };
    use sea_query::{Alias, TableName};

    #[test]
    fn the_operator_class_follows_the_column_only_when_it_is_not_the_default() {
        pretty_assertions::assert_eq!(
            CreateGinIndex::new(
                Alias::new("path_ops"),
                TableName::from(Alias::new("fixture")),
                Alias::new("containment"),
                JsonbOperatorClass::PathOps
            )
            .rendered(),
            r#"CREATE INDEX "path_ops" ON "fixture" USING GIN ("containment" jsonb_path_ops)"#
        );
        pretty_assertions::assert_eq!(
            CreateGinIndex::new(
                Alias::new("default_ops"),
                TableName::from(Alias::new("fixture")),
                Alias::new("existence"),
                JsonbOperatorClass::Default
            )
            .rendered(),
            r#"CREATE INDEX "default_ops" ON "fixture" USING GIN ("existence")"#
        );
    }
}
