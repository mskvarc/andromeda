//! Rendering a schema identifier into a quoted SQL column reference.
//!
//! A column named by an [`Iden`] is turned into the `"col"` or `"table"."col"`
//! text a custom SQL fragment interpolates. A fragment that must name a column
//! inside `cust`/`cust_with_values` — where sea-query cannot render the column
//! for it — references it by the same iden the query builder uses, so a renamed
//! column moves the reference with it rather than leaving a hand-quoted string.

use sea_query::{DynIden, Expr, Iden, IntoIden, SimpleExpr};

/// Quote one identifier: `"name"`.
pub fn quoted(identifier: &dyn Iden) -> String {
    let name = identifier.to_string();
    format!("\"{name}\"")
}

/// Quote a `relation.column` reference: `"relation"."column"`.
pub fn qualified(relation: &dyn Iden, column: &dyn Iden) -> String {
    let relation = relation.to_string();
    let column = column.to_string();
    format!("\"{relation}\".\"{column}\"")
}

/// A reference to one SQL column, carried as the identifiers it is built from.
///
/// A `q` or `scopeQ` predicate reads a column that the driving statement — not
/// this crate — names. Rather than store the rendered `"relation"."column"`
/// string, the reference keeps the [`Iden`]s themselves, so a renamed relation
/// or column moves every reference to it and the quoting is decided in exactly
/// one place, [`ColumnReference::render`].
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

    /// Render the reference as the quoted SQL text a custom fragment interpolates.
    pub fn render(&self) -> String {
        match self {
            ColumnReference::Bare(column) => quoted(&**column),
            ColumnReference::Qualified(relation, column) => qualified(&**relation, &**column),
        }
    }

    /// The reference as a sea-query column expression, so a predicate that used
    /// to interpolate [`render`](Self::render) into a fragment can name the
    /// column through the builder instead.
    pub fn column_expr(&self) -> SimpleExpr {
        match self {
            ColumnReference::Bare(column) => Expr::col(column.clone()).into(),
            ColumnReference::Qualified(relation, column) => Expr::col((relation.clone(), column.clone())).into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::Alias;

    #[test]
    fn one_identifier_is_double_quoted() {
        assert_eq!(quoted(&Alias::new("payload")), "\"payload\"");
    }

    #[test]
    fn a_qualified_reference_quotes_both_halves() {
        assert_eq!(qualified(&Alias::new("e"), &Alias::new("id")), "\"e\".\"id\"");
    }

    #[test]
    fn a_bare_column_reference_renders_the_single_quoted_column() {
        assert_eq!(ColumnReference::bare(Alias::new("payload")).render(), "\"payload\"");
    }

    #[test]
    fn a_qualified_column_reference_renders_both_halves() {
        assert_eq!(ColumnReference::qualified(Alias::new("e"), Alias::new("id")).render(), "\"e\".\"id\"");
    }
}
