//! The function of the `query_name_rewrite` schema the tenant and branch
//! migrations quote the colon names of stored query paths with (ETSI GS CIM
//! 009 v1.9.1 clause 4.9). The schema exists only while a tenant is between
//! the migration that creates it and the one that drops it.
//!
//! No statement of the broker calls it. A rehearsal of the upgrade calls it in
//! a tenant stopped between those migrations, to hold the rewrite to the text
//! each stored path has to become.

use sea_query::{Func, Iden, SimpleExpr};

/// `query_name_rewrite.path_text_with_colon_names_quoted`, written
/// schema-qualified because the schema is on no search path.
#[derive(Iden, Clone, Copy)]
#[iden = "query_name_rewrite.path_text_with_colon_names_quoted"]
struct PathTextWithColonNamesQuoted;

/// The text `path` with each unquoted name holding a colon in double quotes.
#[must_use]
pub fn path_text_with_colon_names_quoted(path: SimpleExpr) -> SimpleExpr {
    Func::cust(PathTextWithColonNamesQuoted).arg(path).into()
}

#[cfg(test)]
mod tests {
    use crate::query_name_rewrite::path_text_with_colon_names_quoted;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    #[test]
    fn the_function_is_called_by_its_schema_qualified_name_with_the_path() {
        let sql = Query::select()
            .expr(path_text_with_colon_names_quoted(Expr::col(Alias::new("written"))))
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"query_name_rewrite.path_text_with_colon_names_quoted("written")"#), "{sql}");
    }
}
