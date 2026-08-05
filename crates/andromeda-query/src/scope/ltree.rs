//! Translation between NGSI-LD scope paths and PostgreSQL `ltree` / `lquery`.

use andromeda_domain::scope::grammar::Scope;
use sea_query::{Expr, SimpleExpr};

/// Convert an NGSI-LD scope path to PostgreSQL ltree format for storage.
///
/// Strips the optional leading `/` and replaces the remaining `/` separators
/// with `.` to form a valid ltree label path. Total by construction: every
/// level of a [`Scope`] already satisfies the clause 4.18 label grammar, which
/// is the same grammar `ltree` accepts.
///
/// # Examples
/// - `/A/B/C` -> `A.B.C`
/// - `/Madrid` -> `Madrid`
pub fn scope_to_ltree(scope: &Scope) -> String {
    // Exactly one leading slash, because clause 4.18 admits no empty level.
    let path = scope.as_str();
    path.strip_prefix('/').unwrap_or(path).replace('/', ".")
}

/// ltree: `col ~ lquery` - tests if ltree matches lquery pattern.
pub fn ltree_matches(col: SimpleExpr, lquery: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("$1 ~ $2::lquery", [col, lquery])
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{PostgresQueryBuilder, Query};

    fn ltree(raw: &str) -> String {
        scope_to_ltree(Scope::new(raw).expect("valid scope"))
    }

    #[test]
    fn a_scope_path_becomes_a_dotted_label_path() {
        assert_eq!(ltree("/A/B/C"), "A.B.C");
        assert_eq!(ltree("/Madrid"), "Madrid");
    }

    #[test]
    fn a_scope_written_without_its_leading_slash_renders_identically() {
        // Clause 4.18: every scope is absolute, so the leading `/` carries no
        // information and both spellings are the same path.
        assert_eq!(ltree("Madrid"), ltree("/Madrid"));
    }

    #[test]
    fn ltree_matches_uses_lquery_cast() {
        let expr = ltree_matches(Expr::col("scope_paths").into(), Expr::val("A.B.*").into());
        let sql = Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains("::lquery"), "{sql}");
    }
}
