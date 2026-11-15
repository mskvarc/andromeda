//! A clause 4.9 Query Term decided over the strings of a `languageMap`.
//!
//! Any one string of a `languageMap` is the target value, so an asserted test
//! (`==`, a range, the ordering operators and `~=`) holds when some string
//! under the selection passes it, and its complement (`!=`, `!~=` and an
//! excluded range) is the failure of that test over a map the instance holds
//! ([`verdict_condition`]). One string is compared the way a String
//! Property's value is compared in its typed column
//! (`crate::q_predicate::typed_column`), so a language map and a plain String
//! answer every operand alike.

use crate::{
    operator::{PatternIndexes, compare, pattern as pattern_predicate},
    q_predicate::{
        language_map::{LanguageMap, LanguageSelection},
        string_ordering::string_ordering_collation,
        typed_column::bound_operand,
        verdict_condition::{ComplementTarget, verdict_condition},
    },
};
use andromeda_ngsi_ld_query::{
    filter::reading::{
        comparator::Comparator,
        interval::Interval,
        operand::Operand,
        order_relation::OrderRelation,
        scalar_test::ScalarTest,
        target_kind::TargetKind,
        term_reading::Test,
        value_test::ValueTest,
    },
    pattern::Pattern,
};
use andromeda_postgresql_seaquery::{
    collation::{PostgresCollation, collate},
    literal::false_literal,
};
use sea_query::{Condition, SimpleExpr};

/// Whether the strings of `map` under `selection` satisfy `test`: existence
/// asks for a string under the selection.
#[must_use]
pub fn language_term(test: &Test, map: &LanguageMap, selection: LanguageSelection<'_>) -> SimpleExpr {
    match test {
        Test::Exists => map.holds(selection),
        Test::Value(value) => language_value_term(value, map, selection).into(),
    }
}

/// Whether the strings of `map` under `selection` satisfy `value`.
///
/// A complement ranges over a map the instance holds in any language, whose
/// target element, the `LanguageProperty`, the matching Entity contains:
/// clause 4.9 has "No matching value is found as the value of the specified
/// language key" hold for a language the map does not hold.
#[must_use]
pub fn language_value_term(value: &ValueTest, map: &LanguageMap, selection: LanguageSelection<'_>) -> Condition {
    let positive = Condition::all().add(map.some_string(selection, |text| string_test(text, value.test())));
    verdict_condition(value, positive, |_| ComplementTarget::Where(map.holds(LanguageSelection::AnyLanguage)))
}

/// Whether some string of `map` under `selection` lies within `interval`,
/// both bounds included (clause 4.9 `Range`).
#[must_use]
pub fn some_string_in_range(map: &LanguageMap, selection: LanguageSelection<'_>, interval: &Interval) -> SimpleExpr {
    map.some_string(selection, |text| string_in_range(text, interval))
}

/// Whether some string of `map` under `selection` stands in `relation` to
/// `operand`.
#[must_use]
pub fn some_string_ordered(map: &LanguageMap, selection: LanguageSelection<'_>, relation: OrderRelation, operand: &Operand) -> SimpleExpr {
    map.some_string(selection, |text| string_comparison(text, Comparator::Ordered(relation), operand))
}

/// Whether some string of `map` under `selection` is in the language of
/// `pattern`.
#[must_use]
pub fn some_string_matching(map: &LanguageMap, selection: LanguageSelection<'_>, pattern: &Pattern) -> SimpleExpr {
    map.some_string(selection, |text| string_pattern(text, pattern))
}

/// Whether one string passes `asked`: identical to the one operand or to one
/// member of a value list, ordered against the operand, within a range, or in
/// the language of a pattern. A range whose bounds are of two kinds holds for
/// no string.
fn string_test(text: SimpleExpr, asked: &ScalarTest) -> SimpleExpr {
    match asked {
        ScalarTest::EqualsAny(operands) => match operands.as_slice() {
            [operand] => string_comparison(text, Comparator::Equal, operand),
            operands => operands
                .iter()
                // clone: the one string is compared against every member
                .fold(Condition::any(), |any, operand| {
                    any.add(string_comparison(text.clone(), Comparator::Equal, operand))
                })
                .into(),
        },
        ScalarTest::Ordered { relation, operand } => string_comparison(text, Comparator::Ordered(*relation), operand),
        ScalarTest::Within(interval) => string_in_range(text, interval),
        ScalarTest::Matches(pattern) => string_pattern(text, pattern),
        ScalarTest::Unsatisfiable => false_literal(),
    }
}

/// Whether one string lies within `interval`, both bounds included.
fn string_in_range(text: SimpleExpr, interval: &Interval) -> SimpleExpr {
    Condition::all()
        // clone: the one string is compared against both bounds
        .add(string_comparison(
            text.clone(),
            Comparator::Ordered(OrderRelation::GreaterOrEqual),
            interval.min(),
        ))
        .add(string_comparison(text, Comparator::Ordered(OrderRelation::LessOrEqual), interval.max()))
        .into()
}

/// Compare one string against one operand.
///
/// A `languageMap` holds `rdf:langString` values, each a String target, so
/// an operand of another data type, which clause 4.9 makes a non-match,
/// compares with none. A String ordering compares under the collation
/// [`string_ordering_collation`] picks, any other comparison under the String
/// column's byte order.
fn string_comparison(text: SimpleExpr, comparator: Comparator, operand: &Operand) -> SimpleExpr {
    if !operand.compares_with(TargetKind::String, comparator.comparison()) {
        return false_literal();
    }
    // The collation the stored String columns are declared under, so a string
    // read out of a map orders exactly as one read from `value_string` does.
    let collation = string_ordering_collation(operand, comparator).unwrap_or_else(PostgresCollation::byte_order);
    compare(collate(text, &collation), comparator, bound_operand(operand, TargetKind::String))
}

/// Whether one string is in the language of a pattern.
fn string_pattern(text: SimpleExpr, pattern: &Pattern) -> SimpleExpr {
    pattern_predicate(text, pattern, PatternIndexes::None)
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::{
        language_map::{LanguageMap, LanguageSelection},
        language_term::language_term,
        lowered_filter::term_of,
    };
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use sea_query::{Alias, Expr as SeaExpr, Query, SelectStatement};

    /// The term of a one-term filter over a stand-in map.
    fn render(q: &str, selection: LanguageSelection<'_>) -> SelectStatement {
        let map = LanguageMap::of_column(SeaExpr::col(Alias::new("map")));
        Query::select().expr(language_term(term_of(q).test(), &map, selection)).to_owned()
    }

    /// Clause 4.9 recommends the UCA for ordering text strings, under the ICU
    /// root collation clause 4.23.1 makes the default.
    #[test]
    fn a_string_ordering_compares_under_the_icu_root_collation() {
        let statement = render("\"https://example.com/name\">\"m\"", LanguageSelection::AnyLanguage);
        assert_statement_snapshot!("language-term-a-string-ordering-compares-under-the-icu-root-collation", statement);
    }

    /// RFC 8259 section 8.3 makes String equality code unit identity, the byte
    /// order of the stored String columns.
    #[test]
    fn a_string_equality_compares_under_the_byte_order_of_the_string_columns() {
        let statement = render("\"https://example.com/name\"==\"m\"", LanguageSelection::AnyLanguage);
        assert_statement_snapshot!("language-term-a-string-equality-compares-under-the-byte-order-of-the-string-columns", statement);
    }

    /// A number or a boolean is a datatype no string carries.
    #[test]
    fn a_number_or_boolean_operand_matches_no_string() {
        for (index, q) in ["\"https://example.com/name\">5", "\"https://example.com/name\"==true"].into_iter().enumerate() {
            assert_statement_snapshot!(
                format!("language-term-a-number-or-boolean-operand-matches-no-string-{}", index + 1),
                render(q, LanguageSelection::AnyLanguage)
            );
        }
    }

    /// The complementary operators hold over a map with a string under the
    /// selection when no string satisfies the positive form.
    #[test]
    fn the_complementary_operators_deny_every_string() {
        for (index, q) in [
            "\"https://example.com/name\"!~=\"^D\"",
            "\"https://example.com/name\"!=\"a\"..\"m\"",
            "\"https://example.com/name\"!=\"Door\"",
        ]
        .into_iter()
        .enumerate()
        {
            assert_statement_snapshot!(
                format!("language-term-the-complementary-operators-deny-every-string-{}", index + 1),
                render(q, LanguageSelection::AnyLanguage)
            );
        }
    }

    /// The positive operators ask for one string that satisfies them.
    #[test]
    fn the_positive_operators_ask_for_one_string() {
        for (index, q) in [
            "\"https://example.com/name\"~=\"^D\"",
            "\"https://example.com/name\"==\"a\"..\"m\"",
            "\"https://example.com/name\"<=\"m\"",
            "\"https://example.com/name\"==\"Door\",\"Gate\"",
        ]
        .into_iter()
        .enumerate()
        {
            assert_statement_snapshot!(
                format!("language-term-the-positive-operators-ask-for-one-string-{}", index + 1),
                render(q, LanguageSelection::AnyLanguage)
            );
        }
    }
}
