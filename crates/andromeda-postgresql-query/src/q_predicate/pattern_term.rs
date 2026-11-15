//! The pattern Query Terms (`~=`/`!~=`, clause 4.9 `patternOp` and
//! `notPatternOp`) compiled against one stored instance.
//!
//! Both polarities compile *inside* the probe of the instance, so both require
//! the attribute to be present. Clause 4.9 is explicit that the entity "shall
//! contain the target element" for `notPatternOp`, so the existence test is
//! not wrapped in `NOT (...)`, which would match entities that simply lacked
//! the attribute.

use crate::{
    operator::{PatternIndexes, pattern as pattern_predicate},
    q_predicate::{
        instance_row::InstanceRow,
        language_map::{LanguageMap, LanguageSelection},
        language_term::language_term,
        typed_column::as_string,
        value_column::ValueColumn,
    },
};
use andromeda_ngsi_ld_query::{
    filter::predicate::{PatternPolarity, Predicate},
    pattern::Pattern,
};
use andromeda_postgresql_stored_values::value_placement::ValueField;
use sea_query::{Condition, SimpleExpr};

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

/// `pred`, a pattern term of `polarity`, tested against one target of `row`.
///
/// A String field holds one string, which the pattern tests directly, and
/// only when the value is a String: a `Date` and a `Time` share the field, and
/// clause 4.9 makes a target "considered as not matching" under both pattern
/// operators "If the target value data type is different than String". Some
/// string of a `languageMap` has to match `~=`, and none may match `!~=`.
#[must_use]
pub fn pattern_on(target: PatternTarget, pred: &Predicate, pattern: &Pattern, polarity: PatternPolarity, row: InstanceRow) -> SimpleExpr {
    match target {
        PatternTarget::Column(column) => as_string(pattern_predicate(row.value(column), polarity, pattern, column_indexes(row, column)), row),
        PatternTarget::LanguageMap => language_term(pred, &LanguageMap::of_instance(row), LanguageSelection::AnyLanguage),
    }
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
pub(crate) fn pattern_value(pred: &Predicate, pattern: &Pattern, polarity: PatternPolarity, row: InstanceRow) -> Condition {
    pattern_targets(row).fold(Condition::any(), |any, target| any.add(pattern_on(target, pred, pattern, polarity, row)))
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::{
        instance_row::InstanceRow,
        pattern_term::{PatternTarget, pattern_targets, pattern_value},
        value_column::ValueColumn,
    };
    use andromeda_ngsi_ld_query::filter::{
        expression::Expr,
        parser::parse,
        predicate::{PatternPolarity, Predicate},
    };
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::value_placement::ValueField;
    use pretty_assertions::assert_eq;
    use sea_query::{Query, SelectStatement};

    fn render(q: &str, polarity: PatternPolarity, row: InstanceRow) -> SelectStatement {
        let Expr::Pred(pred) = parse(q).expect("parses") else {
            panic!("a single predicate");
        };
        let (Predicate::MatchPattern { pattern, .. } | Predicate::NotMatchPattern { pattern, .. }) = &pred else {
            panic!("a pattern term");
        };
        Query::select().cond_where(pattern_value(&pred, pattern, polarity, row)).to_owned()
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
        let statement = render("\"https://example.com/name\"~=\"^D\"", PatternPolarity::Match, InstanceRow::Current);
        assert_statement_snapshot!("pattern-term-a-pattern-on-a-current-row-disjoins-every-target", statement);
    }

    /// Clause 4.9 `notPatternOp`: no string of a `languageMap` may match.
    #[test]
    fn a_not_pattern_denies_every_string_of_the_language_map() {
        let statement = render("\"https://example.com/name\"!~=\"^D\"", PatternPolarity::NotMatch, InstanceRow::Current);
        assert_statement_snapshot!("pattern-term-a-not-pattern-denies-every-string-of-the-language-map", statement);
    }

    /// A Relationship's object is never a pattern target (clause 4.9).
    #[test]
    fn a_pattern_never_reads_a_relationship_object() {
        let statement = render("\"https://example.com/owner\"~=\"urn:Person:.*\"", PatternPolarity::Match, InstanceRow::Current);
        assert_statement_snapshot!("pattern-term-a-pattern-never-reads-a-relationship-object", statement);
    }
}
