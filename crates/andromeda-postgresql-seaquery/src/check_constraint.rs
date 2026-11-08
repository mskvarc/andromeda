//! Adding a `CHECK` constraint to an existing table.
//!
//! sea-query's `ALTER TABLE` adds columns, indexes and foreign keys, but no
//! check constraint. The condition is rendered by the builder that renders
//! every statement, its values inlined, because PostgreSQL takes no parameter in
//! a utility command.

use crate::{
    command::{Command, rendering::RenderedCommand},
    identifier::{quoted_iden, quoted_table},
};
use sea_query::{DynIden, IntoIden, PostgresQueryBuilder, Query, SimpleExpr, TableName};

/// Whether the constraint is checked against the rows the table already holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExistingRows {
    /// The existing rows must satisfy the condition.
    Checked,
    /// Only rows written from now on are checked (`NOT VALID`).
    Unchecked,
}

/// `ALTER TABLE <table> ADD CONSTRAINT <name> CHECK (<condition>) [NOT VALID]`.
#[derive(Clone, Debug)]
pub struct AddCheckConstraint {
    /// The table the constraint is added to.
    table: TableName,
    /// The constraint's name.
    name: DynIden,
    /// The condition every checked row satisfies.
    condition: SimpleExpr,
    /// Whether the rows the table already holds are checked.
    existing_rows: ExistingRows,
}

impl AddCheckConstraint {
    /// A constraint named `name` on `table` holding `condition`.
    pub fn new(table: impl Into<TableName>, name: impl IntoIden, condition: SimpleExpr, existing_rows: ExistingRows) -> AddCheckConstraint {
        AddCheckConstraint {
            table: table.into(),
            name: name.into_iden(),
            condition,
            existing_rows,
        }
    }

    /// The condition as the text the constraint states it in.
    fn condition_text(&self) -> String {
        let select = Query::select().expr(self.condition.clone()).to_string(PostgresQueryBuilder);
        select.strip_prefix("SELECT ").unwrap_or(&select).to_owned()
    }
}

impl RenderedCommand for AddCheckConstraint {
    fn rendered(&self) -> String {
        let validity = match self.existing_rows {
            ExistingRows::Checked => "",
            ExistingRows::Unchecked => " NOT VALID",
        };
        format!(
            "ALTER TABLE {} ADD CONSTRAINT {} CHECK ({}){validity}",
            quoted_table(&self.table),
            quoted_iden(&self.name),
            self.condition_text()
        )
    }
}

impl Command for AddCheckConstraint {}

#[cfg(test)]
mod tests {
    use crate::{
        check_constraint::{AddCheckConstraint, ExistingRows},
        command::rendering::RenderedCommand,
    };
    use sea_query::{Alias, Expr, ExprTrait, TableName};

    #[test]
    fn an_unchecked_constraint_leaves_the_existing_rows_alone() {
        pretty_assertions::assert_eq!(
            AddCheckConstraint::new(
                TableName::from(Alias::new("jsonld_contexts")),
                Alias::new("refuses_implicit"),
                Expr::col(Alias::new("kind")).ne(3),
                ExistingRows::Unchecked
            )
            .rendered(),
            r#"ALTER TABLE "jsonld_contexts" ADD CONSTRAINT "refuses_implicit" CHECK ("kind" <> 3) NOT VALID"#
        );
    }

    #[test]
    fn a_checked_constraint_states_no_validity() {
        pretty_assertions::assert_eq!(
            AddCheckConstraint::new(
                TableName::from(Alias::new("t")),
                Alias::new("c"),
                Expr::col(Alias::new("n")).gt(0),
                ExistingRows::Checked
            )
            .rendered(),
            r#"ALTER TABLE "t" ADD CONSTRAINT "c" CHECK ("n" > 0)"#
        );
    }
}
