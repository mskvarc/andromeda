//! The ordering Query Terms (`>`/`>=`/`<`/`<=`, clause 4.9) compiled against
//! one stored instance.

use crate::q_predicate::{
    instance_row::InstanceRow,
    language_map::{LanguageMap, LanguageSelection},
    language_term::some_string_ordered,
    own_value_arms::OwnValueArms,
    typed_column::{typed_column_condition, typed_field_keys},
};
use andromeda_ngsi_ld_query::filter::reading::{
    comparator::Comparator,
    operand::{Comparison, Operand},
    order_relation::OrderRelation,
    target_kind::TargetKind,
};
use sea_query::Condition;

/// Compare the instance's own value: the typed field holding the operand's
/// kind, and, for an operand a String compares with, every string of a
/// `languageMap`, each of which clause 4.9 makes a target value when no target
/// language is given.
///
/// A number or a boolean is compared against its typed field alone, which no
/// string can satisfy anyway, so a row's probe stays on that column's index.
#[must_use]
pub(crate) fn ordering_value(relation: OrderRelation, operand: &Operand, row: InstanceRow) -> Condition {
    ordering_arms(relation, operand, row).either()
}

/// The readings [`ordering_value`] disjoins.
#[must_use]
pub(crate) fn ordering_arms(relation: OrderRelation, operand: &Operand, row: InstanceRow) -> OwnValueArms {
    let language = operand
        .compares_with(TargetKind::String, Comparison::Order)
        .then(|| some_string_ordered(&LanguageMap::of_instance(row), LanguageSelection::AnyLanguage, relation, operand));
    OwnValueArms {
        typed: Condition::all().add(typed_column_condition(operand, Comparator::Ordered(relation), row)),
        keys: typed_field_keys(operand, Comparator::Ordered(relation), row),
        language,
    }
}
