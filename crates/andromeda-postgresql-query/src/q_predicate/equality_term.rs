//! The equality Query Terms (`==`/`!=`, clause 4.9) compiled against one
//! stored instance.

use crate::{
    operator::does_not_hold,
    q_predicate::{
        instance_row::InstanceRow,
        language_map::{LanguageMap, LanguageSelection},
        language_term::{StringComparability, some_string_in_range, string_comparability},
        structured_target::{StructuredTarget, carries_operand},
        typed_column::typed_column_condition,
        value_column::ValueColumn,
    },
};
use andromeda_ngsi_ld_query::filter::{
    predicate::{CmpOp, SetMembership},
    scalar::{CompareValue, Scalar},
};
use sea_query::{Condition, SimpleExpr};

/// An equality comparison (`==`/`!=`) against the fields of one instance that
/// can hold the clause 4.9 target value.
#[must_use]
pub(crate) fn equality_value(target: StructuredTarget<'_>, membership: SetMembership, rhs: &CompareValue, row: InstanceRow) -> Condition {
    match rhs {
        CompareValue::Scalar(scalar) => equality_condition(target, scalar, membership, row),
        CompareValue::Range { min, max } => range_condition(target, min, max, membership, row),
        // Clause 4.9: every member of a value list is compared with the equality
        // operator, so `==` holds when any of them matches and `!=` when none
        // does. The negated form is a conjunction of per-member inequalities
        // rather than `NOT (any member matches)`: only one value field of an
        // instance is ever populated, so the other fields' comparisons are NULL,
        // and negating the whole disjunction turns that NULL into a non-match for
        // every entity.
        CompareValue::ValueList(values) => match membership {
            SetMembership::Within => values.iter().fold(Condition::any(), |cond, value| {
                cond.add(equality_condition(target, value, SetMembership::Within, row))
            }),
            SetMembership::Outside => values.iter().fold(Condition::all(), |cond, value| {
                cond.add(equality_condition(target, value, SetMembership::Outside, row))
            }),
        },
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
///
/// With a target language the typed columns are left out entirely. A
/// language-selected path addresses a `languageMap` key, and a scalar-valued
/// attribute of the same name carries no map: comparing its typed column would
/// let `attr[en]!=5` match a plain numeric Property, whose target element for
/// that path does not exist at all. That polarity therefore requires a
/// `LanguageProperty`, though not the key: clause 4.9 matches a map where "No
/// matching value is found as the value of the specified language key".
fn equality_condition(target: StructuredTarget<'_>, scalar: &Scalar, membership: SetMembership, row: InstanceRow) -> Condition {
    let column = row.value(ValueColumn::Object);
    match target {
        StructuredTarget::AnyValue => match membership {
            SetMembership::Within => carries_any_target_value(&column, scalar, row),
            SetMembership::Outside => Condition::all().add(does_not_hold(carries_any_target_value(&column, scalar, row))),
        },
        StructuredTarget::TargetLanguage(language) => match membership {
            SetMembership::Within => carries_operand(&column, target, scalar),
            SetMembership::Outside => Condition::all()
                .add(LanguageMap::of_instance(row).holds(LanguageSelection::AnyLanguage))
                .add(does_not_hold(carries_operand(&column, StructuredTarget::TargetLanguage(language), scalar))),
        },
    }
}

/// Whether any column that can hold an unqualified clause 4.9 target value
/// holds this operand.
fn carries_any_target_value(column: &SimpleExpr, scalar: &Scalar, row: InstanceRow) -> Condition {
    Condition::any()
        .add(typed_column_condition(scalar, CmpOp::Equal, row))
        .add(carries_operand(column, StructuredTarget::AnyValue, scalar))
}

/// Compile a range operand (`==10..20` / `!=10..20`).
///
/// A range is two ordering comparisons, which JSONB containment cannot express,
/// so the `languageMap` of a `LanguageProperty` is read string by string
/// instead: clause 4.9 makes each of its strings — every string with no target
/// language, that language's strings with one — a target value of its own.
///
/// With no target language the negated form is the total negation of the
/// interval test, for the reason [`equality_condition`] gives: clause 4.9 makes
/// a target whose datatype is not the range's unequal to it, and a comparison
/// the column cannot even evaluate is unknown rather than false. A language
/// map's strings take part in that interval test, so a map holding a string in
/// the interval is not outside it. With a target language the excluded range
/// requires the `LanguageProperty`, as `!=` does, not the language.
fn range_condition(target: StructuredTarget<'_>, min: &Scalar, max: &Scalar, membership: SetMembership, row: InstanceRow) -> Condition {
    match target {
        StructuredTarget::AnyValue => {
            let within = within_range(min, max, row);
            match membership {
                SetMembership::Within => within,
                SetMembership::Outside => Condition::all().add(does_not_hold(within)),
            }
        }
        StructuredTarget::TargetLanguage(language) => {
            let map = LanguageMap::of_instance(row);
            let selection = LanguageSelection::TargetLanguage(language);
            let in_range = some_string_in_range(&map, selection, min, max);
            match membership {
                SetMembership::Within => Condition::all().add(in_range),
                SetMembership::Outside => Condition::all()
                    .add(map.holds(LanguageSelection::AnyLanguage))
                    .add(Condition::all().add(in_range).not()),
            }
        }
    }
}

/// Whether the target value sits inside the interval, both bounds included
/// (clause 4.9 `Range`): the typed column holding the operand kind, or — for an
/// operand a string can compare with — one string of a `languageMap`.
fn within_range(min: &Scalar, max: &Scalar, row: InstanceRow) -> Condition {
    let typed = Condition::all()
        .add(typed_column_condition(min, CmpOp::GreaterEq, row))
        .add(typed_column_condition(max, CmpOp::LessEq, row));
    match string_comparability(min) {
        StringComparability::Comparable => {
            Condition::any()
                .add(typed)
                .add(some_string_in_range(&LanguageMap::of_instance(row), LanguageSelection::AnyLanguage, min, max))
        }
        StringComparability::Incomparable => typed,
    }
}
