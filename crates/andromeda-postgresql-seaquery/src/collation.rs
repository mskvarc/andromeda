//! The `COLLATE` postfix, which sea-query has no native form for.
//!
//! A collation is a SQL identifier, not a value, so it cannot be bound as a
//! parameter and is written into the statement text. [`PostgresCollation`]
//! carries the name already escaped for the quoted identifier it is written
//! into, so no caller can interpolate a name that breaks out of the quotes.

use sea_query::{Expr, SimpleExpr};

/// A PostgreSQL collation, held as the body of the quoted identifier that names
/// it.
///
/// A collation object is addressed by name, and PostgreSQL quotes that name;
/// the escaping the quoted form needs is applied once here so the name a
/// statement writes is always safe, whatever the client asked for.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PostgresCollation(String);

impl PostgresCollation {
    /// The collation PostgreSQL knows under `name`.
    ///
    /// Escaping rather than rejecting: a quoted identifier's only special
    /// character is the double quote, which the SQL standard escapes by
    /// doubling, so every name has a spelling and none of them can terminate
    /// the identifier early.
    #[must_use]
    pub fn new(name: &str) -> PostgresCollation {
        PostgresCollation(name.replace('"', "\"\""))
    }

    /// The escaped identifier body, as it is written between the quotes.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// `(expr) COLLATE "collation"` - compare `expr` under a named collation.
///
/// `expr` has to yield a collatable type — `text`, or an array of it.
/// PostgreSQL supports no collation on `jsonb`, so a value read out of a
/// payload reaches this already extracted as text (`->>`, `#>>`), never cast:
/// `'"a"'::jsonb::text` keeps the JSON quotes and would sort by them.
///
/// The operand is parenthesised because `COLLATE` binds tighter than every
/// infix operator, so an unparenthesised extraction or `CASE` would collate
/// only its final operand.
#[must_use]
pub fn collate(expr: SimpleExpr, collation: &PostgresCollation) -> SimpleExpr {
    Expr::cust_with_exprs(format!("($1) COLLATE \"{collation}\"", collation = collation.as_str()), [expr])
}

#[cfg(test)]
mod tests {
    use crate::collation::{PostgresCollation, collate};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_collation_is_applied_as_a_quoted_postfix() {
        let sql = render(collate(Expr::col(Alias::new("id")), &PostgresCollation::new("und-u-ks-identic")));
        assert!(sql.contains(r#"COLLATE "und-u-ks-identic""#), "{sql}");
    }

    /// `COLLATE` binds tighter than any infix operator, so the operand keeps
    /// its own parentheses and the collation applies to the whole expression.
    #[test]
    fn a_compound_operand_is_parenthesised_before_the_collation() {
        let extraction = Expr::cust(r#""payload" ->> 'name'"#);
        let sql = render(collate(extraction, &PostgresCollation::new("und-x-icu")));
        assert!(sql.contains(r#"("payload" ->> 'name') COLLATE "und-x-icu""#), "{sql}");
    }

    /// A double quote in a collation name is doubled, so it cannot close the
    /// identifier and let the rest of the name be read as SQL.
    #[test]
    fn a_double_quote_in_a_collation_name_cannot_close_the_identifier() {
        let sql = render(collate(Expr::col(Alias::new("id")), &PostgresCollation::new("a\" OR TRUE --")));
        assert!(sql.contains(r#"COLLATE "a"" OR TRUE --""#), "{sql}");
        assert_eq!(sql.matches('"').count() % 2, 0, "unbalanced quoted identifier: {sql}");
    }
}
