//! The pattern Query Term (`~=`, clause 4.9 `patternOp`) asserted against
//! one stored instance, and the String targets its complement (`!~=`,
//! `notPatternOp`) ranges over.
//!
//! Both compile *inside* the probe of the instance, so both require the
//! attribute to be present. Clause 4.9 is explicit that the entity "shall
//! contain the target element" for `notPatternOp`, so the complement is the
//! failure of the pattern over a String target the instance holds, not a
//! `NOT (...)` around the probe, which would match entities that simply
//! lacked the attribute.

use crate::{
    operator::{PatternIndexes, pattern as pattern_predicate},
    q_predicate::{
        instance_row::InstanceRow,
        language_map::{LanguageMap, LanguageSelection},
        language_term::some_string_matching,
        typed_column::{as_string, holds_string},
        value_column::ValueColumn,
    },
};
use andromeda_ngsi_ld_query::{filter::reading::value_test::Polarity, pattern::Pattern};
use andromeda_postgresql_seaquery::literal::false_literal;
use andromeda_postgresql_stored_values::value_placement::ValueField;
use sea_query::{Condition, ExprTrait, SimpleExpr};

/// Where an instance's own value holds a String a pattern is tested against.
///
/// Only the string fields are targets. Clause 4.9 requires a Relationship
/// operand to non-match every operator other than equality and inequality,
/// and the pattern operators are defined over String targets, so a
/// Relationship's object, which is the whole content of `value_uri`, is never
/// a pattern target: probing it would make `owner~="urn:Person:.*"` select
/// entities the standard requires it to reject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternTarget {
    /// One String field of the instance: on a row `value_string` or the
    /// overflow `value_long_text`, of which a row's value sits in exactly one.
    Column(ValueColumn),
    /// The strings of a `LanguageProperty`'s `languageMap`, held in
    /// `value_object`, each of which clause 4.9 makes a String target value
    /// when no target language is given.
    LanguageMap,
}

/// Every target an instance's own value offers a pattern: its String fields,
/// then its `languageMap`.
pub fn pattern_targets(row: InstanceRow) -> impl Iterator<Item = PatternTarget> {
    row.columns(ValueField::String).map(PatternTarget::Column).chain([PatternTarget::LanguageMap])
}

/// `pattern` tested against one target of `row`.
///
/// A String field holds one string, which the pattern tests directly, and
/// only when the value is a String: a `Date` and a `Time` share the field, and
/// clause 4.9 makes a target "considered as not matching" under both pattern
/// operators "If the target value data type is different than String". Some
/// string of a `languageMap` has to match.
#[must_use]
pub fn pattern_on(target: PatternTarget, pattern: &Pattern, row: InstanceRow) -> SimpleExpr {
    match target {
        PatternTarget::Column(column) => as_string(
            pattern_predicate(row.value(column), Polarity::Asserted, pattern, column_indexes(row, column)),
            row,
        ),
        PatternTarget::LanguageMap => some_string_matching(&LanguageMap::of_instance(row), LanguageSelection::AnyLanguage, pattern),
    }
}

/// Whether the instance's own value is a String target: a String in its
/// String field, or a `languageMap`, each string of which is one. The
/// complement of a pattern ranges over these alone.
#[must_use]
pub(crate) fn string_target(row: InstanceRow) -> SimpleExpr {
    [holds_string(row), LanguageMap::of_instance(row).holds(LanguageSelection::AnyLanguage)]
        .into_iter()
        .reduce(ExprTrait::or)
        .unwrap_or_else(false_literal)
}

/// The indexes over one String field of `row`: a `current_attributes` row
/// keys `value_string` in a B-tree and both String columns in a trigram index,
/// and nothing else indexes a String a pattern reads.
const fn column_indexes(row: InstanceRow, column: ValueColumn) -> PatternIndexes {
    match (row, column) {
        (InstanceRow::Current, ValueColumn::Field(ValueField::String)) => PatternIndexes::TrigramsAndByteOrder,
        (InstanceRow::Current, ValueColumn::LongText) => PatternIndexes::Trigrams,
        (
            InstanceRow::Current,
            ValueColumn::Field(ValueField::Number | ValueField::Boolean | ValueField::Geometry | ValueField::Object | ValueField::DateTime | ValueField::Uri)
            | ValueColumn::LongUri,
        ) => PatternIndexes::None,
        (InstanceRow::History | InstanceRow::Element(_), _unindexed) => PatternIndexes::None,
    }
}

/// A pattern over the instance's own value: the disjunction of its test
/// against every [`PatternTarget`]. A field the value does not occupy is NULL,
/// so the disjunction reads the one that holds it.
///
/// The language-map arm reads `value_object` through an unnested subquery, so
/// the disjunction has no index for every arm and the planner probes each
/// Entity rather than forming a `BitmapOr`. A `~=` conjunct is offered to the
/// page through the trigram indexes as well, one target at a time (see
/// [`keyed_pattern`](crate::q_predicate::keyed_pattern)).
#[must_use]
pub(crate) fn pattern_value(pattern: &Pattern, row: InstanceRow) -> Condition {
    pattern_targets(row).fold(Condition::any(), |any, target| any.add(pattern_on(target, pattern, row)))
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::{
        instance_row::InstanceRow,
        lowered_filter::term_of,
        pattern_term::{PatternTarget, pattern_targets, pattern_value, string_target},
        value_column::ValueColumn,
    };
    use andromeda_ngsi_ld_query::filter::reading::{scalar_test::ScalarTest, term_reading::Test};
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::value_placement::ValueField;
    use pretty_assertions::assert_eq;
    use sea_query::{Query, SelectStatement};

    fn render(q: &str, row: InstanceRow) -> SelectStatement {
        let term = term_of(q);
        let Test::Value(value) = term.test() else {
            panic!("a value test");
        };
        let ScalarTest::Matches(pattern) = value.test() else {
            panic!("a pattern term");
        };
        Query::select().cond_where(pattern_value(pattern, row)).to_owned()
    }

    #[test]
    fn a_current_row_offers_both_string_columns_and_its_language_map() {
        assert_eq!(
            pattern_targets(InstanceRow::Current).collect::<Vec<_>>(),
            [
                PatternTarget::Column(ValueColumn::Field(ValueField::String)),
                PatternTarget::Column(ValueColumn::LongText),
                PatternTarget::LanguageMap
            ]
        );
        assert_eq!(
            pattern_targets(InstanceRow::History).collect::<Vec<_>>(),
            [PatternTarget::Column(ValueColumn::Field(ValueField::String)), PatternTarget::LanguageMap]
        );
    }

    #[test]
    fn a_pattern_on_a_current_row_disjoins_every_target() {
        let statement = render("\"https://example.com/name\"~=\"^D\"", InstanceRow::Current);
        assert_statement_snapshot!("pattern-term-a-pattern-on-a-current-row-disjoins-every-target", statement);
    }

    /// Clause 4.9 `notPatternOp` makes a target of another data type than
    /// String not matching, so its complement ranges over a String the
    /// instance holds or its `languageMap`.
    #[test]
    fn a_not_pattern_ranges_over_a_string_or_a_language_map() {
        for (index, row) in [InstanceRow::Current, InstanceRow::History].into_iter().enumerate() {
            assert_statement_snapshot!(
                format!("pattern-term-a-not-pattern-ranges-over-a-string-or-a-language-map-{}", index + 1),
                Query::select().expr(string_target(row)).to_owned()
            );
        }
    }

    /// A Relationship's object is never a pattern target (clause 4.9).
    #[test]
    fn a_pattern_never_reads_a_relationship_object() {
        let statement = render("\"https://example.com/owner\"~=\"urn:Person:.*\"", InstanceRow::Current);
        assert_statement_snapshot!("pattern-term-a-pattern-never-reads-a-relationship-object", statement);
    }
}
