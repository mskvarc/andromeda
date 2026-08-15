//! The `COLLATE` postfix, which sea-query has no native form for.
//!
//! A collation is a SQL identifier, not a value, so it cannot be bound as a
//! parameter and is written into the statement text. The caller is responsible
//! for passing a validated collation name — a locale identifier with no quoting
//! metacharacters — so the interpolation cannot break out of the quoted name.

use sea_query::{Expr, SimpleExpr};

/// `expr COLLATE "collation"` - compare `expr` under a named collation.
///
/// `collation` must be a validated collation identifier; it is written into the
/// quoted identifier directly, never bound.
pub fn collate(expr: SimpleExpr, collation: &str) -> SimpleExpr {
    Expr::cust_with_exprs(format!("$1 COLLATE \"{collation}\""), [expr])
}

#[cfg(test)]
mod tests {
    use crate::collation::collate;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    #[test]
    fn a_collation_is_applied_as_a_quoted_postfix() {
        let sql = Query::select()
            .expr(collate(Expr::col(Alias::new("id")).into(), "und-u-ks-identic"))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"COLLATE "und-u-ks-identic""#), "{sql}");
    }
}
