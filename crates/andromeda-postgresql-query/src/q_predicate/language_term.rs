//! A clause 4.9 Query Term decided over the strings of a `languageMap`.
//!
//! Each operator is the positive or the complementary existential of
//! [`LanguageMap`]: `==`, a range, the ordering operators and `~=` hold when
//! some string under the selection satisfies them; `!=`, `!~=` and an excluded
//! range when the map exists and no string under the selection does. One string is
//! compared the way a String Property's value is compared in its typed column
//! (`crate::q_predicate::typed_column`), so a language map and a plain String
//! answer every operand alike.

use crate::{
    jsonb::lift_comparison,
    operator::{compare, pattern as pattern_predicate},
    q_predicate::{
        language_map::{LanguageMap, LanguageSelection},
        string_ordering::string_ordering_collation,
    },
};
use andromeda_ngsi_ld_query::{
    filter::{
        predicate::{CmpOp, PatternPolarity, Predicate, SetMembership},
        scalar::{CompareValue, Scalar},
    },
    pattern::Pattern,
};
use andromeda_postgresql_seaquery::{
    collation::{PostgresCollation, collate},
    literal::false_literal,
};
use sea_query::{Condition, Expr as SeaExpr, SimpleExpr};

/// The collation the stored String columns are declared under, so a string read
/// out of a map orders exactly as one read from `value_string` does.
const BYTE_ORDER: &str = "C";

/// Whether a string can compare with an operand at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringComparability {
    /// A String, `Date`, `Time` or `DateTime` operand, which a string may
    /// equal or order against.
    Comparable,
    /// A number or a boolean: a datatype no string carries, which clause 4.9
    /// makes a non-match under every operator but `!=`.
    Incomparable,
}

/// Whether a string can compare with `scalar`.
#[must_use]
pub const fn string_comparability(scalar: &Scalar) -> StringComparability {
    match scalar {
        Scalar::Str(_) | Scalar::Uri(_) | Scalar::Date(_) | Scalar::Time(_) | Scalar::DateTime(_) => StringComparability::Comparable,
        Scalar::Number(_) | Scalar::Bool(_) => StringComparability::Incomparable,
    }
}

/// Whether the strings of `map` under `selection` satisfy `pred`.
#[must_use]
pub fn language_term(pred: &Predicate, map: &LanguageMap, selection: LanguageSelection<'_>) -> SimpleExpr {
    match pred {
        Predicate::Exists(_) => map.holds(selection),
        Predicate::Equality { membership, rhs, .. } => {
            let test = |text: SimpleExpr| equality_test(text, rhs);
            match membership {
                SetMembership::Within => map.some_string(selection, test),
                SetMembership::Outside => map.no_string(selection, test),
            }
        }
        Predicate::Ordering { op, rhs, .. } => map.some_string(selection, |text| string_comparison(text, op.as_cmp_op(), rhs)),
        Predicate::MatchPattern { pattern, .. } => map.some_string(selection, |text| string_pattern(text, pattern)),
        Predicate::NotMatchPattern { pattern, .. } => map.no_string(selection, |text| string_pattern(text, pattern)),
    }
}

/// Whether some string of `map` under `selection` lies within a range, both
/// bounds included (clause 4.9 `Range`).
#[must_use]
pub fn some_string_in_range(map: &LanguageMap, selection: LanguageSelection<'_>, min: &Scalar, max: &Scalar) -> SimpleExpr {
    map.some_string(selection, |text| string_in_range(text, min, max))
}

/// The positive form of an equality operand against one string: identical to
/// the scalar, to one member of a value list, or within a range.
fn equality_test(text: SimpleExpr, rhs: &CompareValue) -> SimpleExpr {
    match rhs {
        CompareValue::Scalar(scalar) => string_comparison(text, CmpOp::Equal, scalar),
        CompareValue::Range { min, max } => string_in_range(text, min, max),
        CompareValue::ValueList(values) => values
            .iter()
            // clone: the one string is compared against every member
            .fold(Condition::any(), |any, value| any.add(string_comparison(text.clone(), CmpOp::Equal, value)))
            .into(),
    }
}

/// Whether one string lies within a range, both bounds included.
fn string_in_range(text: SimpleExpr, min: &Scalar, max: &Scalar) -> SimpleExpr {
    Condition::all()
        // clone: the one string is compared against both bounds
        .add(string_comparison(text.clone(), CmpOp::GreaterEq, min))
        .add(string_comparison(text, CmpOp::LessEq, max))
        .into()
}

/// Compare one string against one operand.
///
/// A number or a boolean is a datatype no string carries, which clause 4.9
/// makes a non-match. A String ordering compares under the collation
/// [`string_ordering_collation`] picks. Any other String, `Date` or `Time`
/// comparison runs on the canonical text the String column holds the value in,
/// under that column's byte order. A `DateTime` compares as an instant once the
/// string is shown to be one, which carries no collation. The lifting is
/// [`lift_comparison`]'s, which the JSON value reading shares.
fn string_comparison(text: SimpleExpr, op: CmpOp, scalar: &Scalar) -> SimpleExpr {
    if string_comparability(scalar) == StringComparability::Incomparable {
        return false_literal();
    }
    let lifted = lift_comparison(scalar, text);
    let stored = match (scalar, string_ordering_collation(scalar, op)) {
        (Scalar::DateTime(_), _) => lifted.stored,
        (Scalar::Str(_) | Scalar::Uri(_) | Scalar::Date(_) | Scalar::Time(_) | Scalar::Number(_) | Scalar::Bool(_), Some(collation)) => {
            collate(lifted.stored, &collation)
        }
        (Scalar::Str(_) | Scalar::Uri(_) | Scalar::Date(_) | Scalar::Time(_) | Scalar::Number(_) | Scalar::Bool(_), None) => {
            collate(lifted.stored, &PostgresCollation::new(BYTE_ORDER))
        }
    };
    compare(stored, op, lifted.operand)
}

/// Whether one string is in the language of a pattern.
fn string_pattern(text: SimpleExpr, pattern: &Pattern) -> SimpleExpr {
    pattern_predicate(text, PatternPolarity::Match, SeaExpr::val(pattern.portable().as_str()))
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::{
        language_map::{LanguageMap, LanguageSelection},
        language_term::language_term,
    };
    use andromeda_ngsi_ld_query::filter::{expression::Expr, parser::parse};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr as SeaExpr, PostgresQueryBuilder, Query};

    /// The term of a one-predicate filter over a stand-in map, rendered.
    fn render(q: &str, selection: LanguageSelection<'_>) -> String {
        let Expr::Pred(pred) = parse(q).expect("parses") else {
            panic!("a single predicate");
        };
        let map = LanguageMap::of_column(SeaExpr::col(Alias::new("map")));
        Query::select()
            .expr(language_term(&pred, &map, selection))
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    /// Clause 4.9 recommends the UCA for ordering text strings, under the ICU
    /// root collation clause 4.23.1 makes the default.
    #[test]
    fn a_string_ordering_compares_under_the_icu_root_collation() {
        let sql = render("\"https://example.com/name\">\"m\"", LanguageSelection::AnyLanguage);
        assert!(sql.contains(r#"COLLATE "und-x-icu") > 'm'"#), "{sql}");
    }

    /// RFC 8259 section 8.3 makes String equality code unit identity, the byte
    /// order of the stored String columns.
    #[test]
    fn a_string_equality_compares_under_the_byte_order_of_the_string_columns() {
        let sql = render("\"https://example.com/name\"==\"m\"", LanguageSelection::AnyLanguage);
        assert!(sql.contains(r#"COLLATE "C") = 'm'"#), "{sql}");
    }

    /// A number or a boolean is a datatype no string carries.
    #[test]
    fn a_number_or_boolean_operand_matches_no_string() {
        for q in ["\"https://example.com/name\">5", "\"https://example.com/name\"==true"] {
            assert!(render(q, LanguageSelection::AnyLanguage).contains("FALSE"), "{q}");
        }
    }

    /// The complementary operators hold over a map with a string under the
    /// selection when no string satisfies the positive form.
    #[test]
    fn the_complementary_operators_deny_every_string() {
        for q in [
            "\"https://example.com/name\"!~=\"^D\"",
            "\"https://example.com/name\"!=\"a\"..\"m\"",
            "\"https://example.com/name\"!=\"Door\"",
        ] {
            let sql = render(q, LanguageSelection::AnyLanguage);
            assert!(sql.contains("NOT EXISTS"), "{q}: {sql}");
        }
    }

    /// The positive operators ask for one string that satisfies them.
    #[test]
    fn the_positive_operators_ask_for_one_string() {
        for q in [
            "\"https://example.com/name\"~=\"^D\"",
            "\"https://example.com/name\"==\"a\"..\"m\"",
            "\"https://example.com/name\"<=\"m\"",
            "\"https://example.com/name\"==\"Door\",\"Gate\"",
        ] {
            let sql = render(q, LanguageSelection::AnyLanguage);
            assert!(!sql.contains("NOT "), "{q}: {sql}");
            assert_eq!(sql.matches("EXISTS").count(), 1, "{q}: {sql}");
        }
    }
}
