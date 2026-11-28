//! The equality Query Terms (`==` with a value, a value list or a range,
//! clause 4.9) asserted against one stored instance; a complement of each is
//! their failure ([`verdict_condition`](crate::q_predicate::verdict_condition)).

use crate::q_predicate::{
    instance_row::InstanceRow,
    language_map::{LanguageMap, LanguageSelection},
    language_term::some_string_in_range,
    own_value_arms::OwnValueArms,
    structured_target::carries_operand,
    typed_column::{typed_column_condition, typed_field_conditions, typed_field_keys},
    value_column::ValueColumn,
};
use andromeda_ngsi_ld_query::filter::reading::{
    comparator::Comparator,
    interval::Interval,
    operand::{Comparison, Operand},
    order_relation::OrderRelation,
    target_kind::TargetKind,
};
use andromeda_postgresql_seaquery::literal::false_literal;
use andromeda_postgresql_stored_values::value_placement::ValueField;
use sea_query::{Condition, ExprTrait, SimpleExpr};

/// Whether the instance's own value equals one of `operands` (`==` with a
/// value or a value list), against the fields of one instance that can hold
/// the clause 4.9 target value of a path with no target language.
///
/// Clause 4.9 compares every member of a value list with the equality
/// operator, so the list holds when any of them matches. A target-language
/// path addresses `languageMap` keys instead, and is decided over the map's
/// strings one by one.
#[must_use]
pub(crate) fn equals_any_value(operands: &[Operand], row: InstanceRow) -> Condition {
    match operands {
        [operand] => carries_any_target_value(operand, row),
        operands => operands
            .iter()
            .fold(Condition::any(), |cond, operand| cond.add(carries_any_target_value(operand, row))),
    }
}

/// Whether any column that can hold an unqualified clause 4.9 target value
/// holds `operand`.
///
/// A scalar target lives in one typed column; a `LanguageProperty`'s
/// `languageMap`, a `ListProperty`'s `valueList`, a `ListRelationship`'s
/// `objectList`, a multi-term `VocabProperty`'s `vocab` and an array-valued
/// Property all live in `value_object`, where an equality "Is included in the
/// target value". Clause 4.9 names each of them a target value, so both
/// readings are tested and disjoined; the row's own check constraint leaves
/// exactly one value column populated per row, so at most one disjunct is
/// anything but NULL. Unequal is the total negation of this same disjunction,
/// so the two operators cannot drift apart.
fn carries_any_target_value(operand: &Operand, row: InstanceRow) -> Condition {
    equality_arms(operand, row).either()
}

/// The readings an equality with one operand disjoins, each answered by an
/// index of its own: every typed field holding a kind the operand compares
/// with, and the structured value.
///
/// The index path of the term reads each arm on its own, so all are built
/// here once and none can drift from the condition the page rechecks.
#[derive(Debug, Clone)]
pub struct EqualityArms {
    /// The equality on each typed field, one field each.
    pub fields: Vec<SimpleExpr>,
    /// The equality on each typed field's columns alone
    /// ([`typed_field_keys`]), one field each, which every row `fields` keeps
    /// passes.
    pub keys: Vec<SimpleExpr>,
    /// The operand carried as the target value of a structured value.
    pub structured: Condition,
}

impl EqualityArms {
    /// The term's condition: some reading holds.
    #[must_use]
    pub fn either(self) -> Condition {
        Condition::any()
            .add(self.fields.into_iter().reduce(ExprTrait::or).unwrap_or_else(false_literal))
            .add(self.structured)
    }
}

/// The arms of an equality of the instance's own value with `operand`.
#[must_use]
pub(crate) fn equality_arms(operand: &Operand, row: InstanceRow) -> EqualityArms {
    let column: SimpleExpr = row.value(ValueColumn::Field(ValueField::Object));
    EqualityArms {
        fields: typed_field_conditions(operand, Comparator::Equal, row),
        keys: typed_field_keys(operand, Comparator::Equal, row),
        structured: carries_operand(&column, &row.value_type_key(), &row.attribute_family(), operand),
    }
}

/// Whether the target value sits inside `interval`, both bounds included
/// (clause 4.9 `Range`).
///
/// A range is two ordering comparisons, which JSONB containment cannot express,
/// so the `languageMap` of a `LanguageProperty` is read string by string
/// instead: clause 4.9 makes each of its strings a target value of its own, so
/// a map holding a string in the interval is inside it.
#[must_use]
pub(crate) fn within_range(interval: &Interval, row: InstanceRow) -> Condition {
    range_arms(interval, row).either()
}

/// The readings [`within_range`] disjoins: the typed column holding the
/// operand kind, or, for bounds a String compares with, one string of a
/// `languageMap`.
#[must_use]
pub(crate) fn range_arms(interval: &Interval, row: InstanceRow) -> OwnValueArms {
    let language = interval
        .min()
        .compares_with(TargetKind::String, Comparison::Order)
        .then(|| some_string_in_range(&LanguageMap::of_instance(row), LanguageSelection::AnyLanguage, interval));
    let lower = Comparator::Ordered(OrderRelation::GreaterOrEqual);
    let upper = Comparator::Ordered(OrderRelation::LessOrEqual);
    OwnValueArms {
        typed: Condition::all()
            .add(typed_column_condition(interval.min(), lower, row))
            .add(typed_column_condition(interval.max(), upper, row)),
        keys: typed_field_keys(interval.min(), lower, row)
            .into_iter()
            .zip(typed_field_keys(interval.max(), upper, row))
            .map(|(min, max)| min.and(max))
            .collect(),
        language,
    }
}
