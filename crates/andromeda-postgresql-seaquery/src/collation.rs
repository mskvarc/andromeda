//! Collations: the `COLLATE` postfix and the creation of an ICU collation,
//! neither of which sea-query has a native form for.
//!
//! A collation is a SQL identifier, not a value, so it cannot be bound as a
//! parameter and is written into the statement text, quoted by the builder
//! that quotes every identifier, so no name can break out of its quotes.

use crate::{
    command::{Command, rendering::RenderedCommand},
    identifier::quoted_name,
    literal::quoted_text,
    setting::{Setting, SettingAssignment, SettingScope},
};
use sea_query::{Alias, Expr, SimpleExpr};

/// A PostgreSQL collation, named as its collation object is.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PostgresCollation(String);

impl PostgresCollation {
    /// The collation PostgreSQL knows under `name`.
    #[must_use]
    pub fn new(name: &str) -> PostgresCollation {
        PostgresCollation(name.to_owned())
    }

    /// The byte-wise collation, `"C"`, which every stored identifier and text
    /// column is declared under: strings compare and order by their bytes.
    #[must_use]
    pub fn byte_order() -> PostgresCollation {
        PostgresCollation::new("C")
    }

    /// The collation's name as a quoted identifier.
    fn quoted(&self) -> String {
        quoted_name(&Alias::new(self.0.as_str()))
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
    Expr::cust_with_exprs(format!("($1) COLLATE {}", collation.quoted()), [expr])
}

/// `SET LOCAL icu_validation_level = 'error'; CREATE COLLATION IF NOT EXISTS
/// "<name>" (provider = icu, locale = '<locale>')`.
///
/// `icu_validation_level` defaults to `warning`, at which PostgreSQL creates a
/// usable collation object for a locale ICU does not know and every ordering
/// under it silently falls back to the root collation. At `error` the locale
/// is refused with `invalid_parameter_value` instead. The level is `LOCAL`, so
/// the command runs inside a transaction and the connection leaves it with
/// the level it had.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateIcuCollation {
    /// The collation object created.
    name: PostgresCollation,
    /// The ICU locale it collates by.
    locale: String,
}

impl CreateIcuCollation {
    /// Create `name`, collating by the ICU `locale`.
    pub fn new(name: PostgresCollation, locale: impl Into<String>) -> CreateIcuCollation {
        CreateIcuCollation { name, locale: locale.into() }
    }
}

impl RenderedCommand for CreateIcuCollation {
    fn rendered(&self) -> String {
        let strict = SettingAssignment::new(Setting::IcuValidationLevel, "error", SettingScope::Transaction);
        format!(
            "{}; CREATE COLLATION IF NOT EXISTS {} (provider = icu, locale = {})",
            strict.rendered(),
            self.name.quoted(),
            quoted_text(&self.locale)
        )
    }
}

impl Command for CreateIcuCollation {}

#[cfg(test)]
mod tests {
    use crate::{
        collation::{CreateIcuCollation, PostgresCollation, collate},
        command::rendering::RenderedCommand,
    };
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

    #[test]
    fn an_icu_collation_is_created_under_strict_locale_validation() {
        assert_eq!(
            CreateIcuCollation::new(PostgresCollation::new("de-u-co-phonebk"), "de-u-co-phonebk").rendered(),
            r#"SET LOCAL icu_validation_level = 'error'; CREATE COLLATION IF NOT EXISTS "de-u-co-phonebk" (provider = icu, locale = 'de-u-co-phonebk')"#
        );
    }
}
