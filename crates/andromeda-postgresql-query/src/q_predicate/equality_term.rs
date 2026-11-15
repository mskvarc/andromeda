//! The equality Query Terms (`==`/`!=`, clause 4.9) compiled against one
//! stored instance.

use crate::{
    operator::does_not_hold,
    q_predicate::{
        instance_row::InstanceRow,
        language_map::{LanguageMap, LanguageSelection},
        language_term::{StringComparability, some_string_in_range, string_comparability},
        own_value_arms::OwnValueArms,
        structured_target::carries_operand,
        typed_column::typed_column_condition,
        value_column::ValueColumn,
    },
};
use andromeda_ngsi_ld_query::filter::reading::{
    comparator::Comparator,
    interval::Interval,
    operand::Operand,
    order_relation::OrderRelation,
    value_test::Polarity,
};
use andromeda_postgresql_stored_values::value_placement::ValueField;
use sea_query::{Condition, SimpleExpr};

/// An equality with one of `operands` (`==`/`!=` with a value or a value
/// list), under `polarity`, against the fields of one instance that can hold
/// the clause 4.9 target value of a path with no target language.
///
/// A target-language path addresses `languageMap` keys instead, and is decided
/// over the map's strings one by one.
#[must_use]
pub(crate) fn equals_any_value(operands: &[Operand], polarity: Polarity, row: InstanceRow) -> Condition {
    match (operands, polarity) {
        ([operand], Polarity::Asserted | Polarity::Complement) => equality_condition(operand, polarity, row),
        // Clause 4.9: every member of a value list is compared with the equality
        // operator, so `==` holds when any of them matches and `!=` when none
        // does. The negated form is a conjunction of per-member inequalities
        // rather than `NOT (any member matches)`: only one value field of an
        // instance is ever populated, so the other fields' comparisons are NULL,
        // and negating the whole disjunction turns that NULL into a non-match for
        // every entity.
        (operands, Polarity::Asserted) => operands
            .iter()
            .fold(Condition::any(), |cond, operand| cond.add(equality_condition(operand, Polarity::Asserted, row))),
        (operands, Polarity::Complement) => operands.iter().fold(Condition::all(), |cond, operand| {
            cond.add(equality_condition(operand, Polarity::Complement, row))
        }),
    }
}

/// `positive` under `polarity`: a complement is the total negation of the
/// positive condition, for the reason [`equality_condition`] gives.
#[must_use]
pub(crate) fn complemented(polarity: Polarity, positive: Condition) -> Condition {
    match polarity {
        Polarity::Asserted => positive,
        Polarity::Complement => Condition::all().add(does_not_hold(positive)),
    }
}

/// One equality comparison against every column the target value can occupy.
///
/// A scalar target lives in one typed column; a `LanguageProperty`'s
/// `languageMap`, a `ListProperty`'s `valueList`, a `ListRelationship`'s
/// `objectList`, a multi-term `VocabProperty`'s `vocab` and an array-valued
/// Property all live in `value_object`. Clause 4.9 names each of them a target
/// value, so both readings are tested and disjoined; the row's own check
/// constraint leaves exactly one value column populated per row, so at most one
/// disjunct is anything but NULL.
///
/// `unequal` is compiled as the total negation of that same disjunction rather
/// than as a disjunction of negated comparisons, which is what clause 4.9
/// requires of it: "If the data type of the target value and the data type of
/// the Query Term value are different, then they shall be considered unequal."
/// A row whose value sits in another typed column makes every comparison
/// unknown, and only a total negation reads that as the datatype difference it
/// is: `temp!="20"` over a numeric Property matches, as the clause requires.
/// Its inner disjunction is the whole `equal` condition,
/// so the two operators cannot drift apart.
fn equality_condition(operand: &Operand, polarity: Polarity, row: InstanceRow) -> Condition {
    let column = row.value(ValueColumn::Field(ValueField::Object));
    complemented(polarity, carries_any_target_value(&column, operand, row))
}

/// Whether any column that can hold an unqualified clause 4.9 target value
/// holds this operand.
fn carries_any_target_value(column: &SimpleExpr, operand: &Operand, row: InstanceRow) -> Condition {
    Condition::any()
        .add(typed_column_condition(operand, Comparator::Equal, row))
        .add(carries_operand(column, &row.value_type_key(), &row.attribute_family(), operand))
}

/// Compile a range operand (`==10..20` / `!=10..20`) under `polarity`.
///
/// A range is two ordering comparisons, which JSONB containment cannot express,
/// so the `languageMap` of a `LanguageProperty` is read string by string
/// instead: clause 4.9 makes each of its strings a target value of its own.
///
/// The negated form is the total negation of the interval test, for the reason
/// [`equality_condition`] gives: clause 4.9 makes a target whose datatype is
/// not the range's unequal to it, and a comparison the column cannot even
/// evaluate is unknown rather than false. A language map's strings take part in
/// that interval test, so a map holding a string in the interval is not outside
/// it.
#[must_use]
pub(crate) fn range_value(interval: &Interval, polarity: Polarity, row: InstanceRow) -> Condition {
    complemented(polarity, within_range(interval, row))
}

/// Whether the target value sits inside the interval, both bounds included
/// (clause 4.9 `Range`).
fn within_range(interval: &Interval, row: InstanceRow) -> Condition {
    range_arms(interval, row).either()
}

/// The readings [`within_range`] disjoins: the typed column holding the
/// operand kind, or (for an operand a string can compare with) one string of
/// a `languageMap`.
#[must_use]
pub(crate) fn range_arms(interval: &Interval, row: InstanceRow) -> OwnValueArms {
    let language = match string_comparability(interval.min()) {
        StringComparability::Comparable => Some(some_string_in_range(&LanguageMap::of_instance(row), LanguageSelection::AnyLanguage, interval)),
        StringComparability::Incomparable => None,
    };
    OwnValueArms {
        typed: Condition::all()
            .add(typed_column_condition(interval.min(), Comparator::Ordered(OrderRelation::GreaterOrEqual), row))
            .add(typed_column_condition(interval.max(), Comparator::Ordered(OrderRelation::LessOrEqual), row)),
        language,
    }
}
