//! The ordering Query Terms (`>`/`>=`/`<`/`<=`, clause 4.9) compiled against
//! one stored instance.

use crate::q_predicate::{
    instance_row::InstanceRow,
    language_map::{LanguageMap, LanguageSelection},
    language_term::{StringComparability, language_term, string_comparability},
    own_value_arms::OwnValueArms,
    typed_column::typed_column_condition,
};
use andromeda_ngsi_ld_query::filter::{
    predicate::{OrderingOp, Predicate},
    scalar::Scalar,
};
use sea_query::Condition;

/// Compare the instance's own value: the typed field holding the operand's
/// kind, and (for an operand a string can compare with) every string of a
/// `languageMap`, each of which clause 4.9 makes a target value when no target
/// language is given.
///
/// A number or a boolean is compared against its typed field alone, which no
/// string can satisfy anyway, so a row's probe stays on that column's index.
#[must_use]
pub(crate) fn ordering_value(pred: &Predicate, op: OrderingOp, rhs: &Scalar, row: InstanceRow) -> Condition {
    ordering_arms(pred, op, rhs, row).either()
}

/// The readings [`ordering_value`] disjoins.
#[must_use]
pub(crate) fn ordering_arms(pred: &Predicate, op: OrderingOp, rhs: &Scalar, row: InstanceRow) -> OwnValueArms {
    let language = match string_comparability(rhs) {
        StringComparability::Comparable => Some(language_term(pred, &LanguageMap::of_instance(row), LanguageSelection::AnyLanguage)),
        StringComparability::Incomparable => None,
    };
    OwnValueArms {
        typed: Condition::all().add(typed_column_condition(rhs, op.as_cmp_op(), row)),
        language,
    }
}
