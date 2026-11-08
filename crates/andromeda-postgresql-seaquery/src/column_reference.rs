//! A reference to one column, carried as the identifiers it is built from.
//!
//! A `q` or `scopeQ` predicate reads a column that the driving statement, not
//! the predicate, names. The reference keeps the [`sea_query::Iden`]s
//! themselves, so a renamed relation or column moves every reference to it
//! and the builder decides the quoting.

use sea_query::{DynIden, Expr, IntoIden, SimpleExpr};

/// A reference to one SQL column.
#[derive(Debug, Clone, PartialEq)]
pub enum ColumnReference {
    /// A bare column, rendered `"column"`.
    Bare(DynIden),
    /// A `relation.column` reference, rendered `"relation"."column"`.
    Qualified(DynIden, DynIden),
}

impl ColumnReference {
    /// A bare column reference, named by `column`.
    pub fn bare(column: impl IntoIden) -> ColumnReference {
        ColumnReference::Bare(column.into_iden())
    }

    /// A `relation.column` reference, named by `relation` and `column`.
    pub fn qualified(relation: impl IntoIden, column: impl IntoIden) -> ColumnReference {
        ColumnReference::Qualified(relation.into_iden(), column.into_iden())
    }

    /// The reference as a sea-query column expression.
    #[must_use]
    pub fn column_expr(&self) -> SimpleExpr {
        match self {
            ColumnReference::Bare(column) => Expr::col(column.clone()),
            ColumnReference::Qualified(relation, column) => Expr::col((relation.clone(), column.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::column_reference::ColumnReference;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    #[test]
    fn a_bare_reference_names_the_column_alone() {
        let sql = Query::select()
            .expr(ColumnReference::bare(Alias::new("payload")).column_expr())
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT "payload""#);
    }

    #[test]
    fn a_qualified_reference_names_the_relation_and_the_column() {
        let sql = Query::select()
            .expr(ColumnReference::qualified(Alias::new("e"), Alias::new("id")).column_expr())
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT "e"."id""#);
    }
}
