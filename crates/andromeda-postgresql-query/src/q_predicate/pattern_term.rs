//! The pattern Query Terms (`~=`/`!~=`, clause 4.9 `patternOp` and
//! `notPatternOp`) compiled against one stored instance.
//!
//! Both polarities compile *inside* the probe of the instance, so both require
//! the attribute to be present. Clause 4.9 is explicit that the entity "shall
//! contain the target element" for `notPatternOp`, so the existence test is
//! not wrapped in `NOT (...)`, which would match entities that simply lacked
//! the attribute.

use crate::{
    operator::pattern as pattern_predicate,
    q_predicate::{
        instance_row::InstanceRow,
        language_map::{LanguageMap, LanguageSelection},
        language_term::language_term,
    },
};
use andromeda_ngsi_ld_query::{
    filter::predicate::{PatternPolarity, Predicate},
    pattern::Pattern,
};
use sea_query::{Condition, Expr as SeaExpr};

/// A pattern over the instance's own value.
///
/// Only the string fields are tested (on a row `value_string` and the overflow
/// `value_long_text`, as one disjunction: a row's value sits in exactly one of
/// them, so the other arm is NULL and the disjunction reads the one that holds
/// it). Clause 4.9 requires a Relationship operand to non-match every operator
/// other than equality and inequality, and the pattern operators are defined
/// over String targets — so a Relationship's object, which is the whole content
/// of `value_uri`, is never a pattern target. Probing it would
/// make `owner~="urn:Person:.*"` select entities the standard requires it to reject.
///
/// A `LanguageProperty` row holds its strings in `value_object` instead, and
/// clause 4.9 makes each of them a String target value when no target language
/// is given, so its `languageMap` is read beside the string columns: some
/// string has to match `~=`, and none may match `!~=`.
///
/// The attribute-aware trigram index `idx_ca_value_string_trgm` on
/// `(attribute_name_key int4_ops, value_string gin_trgm_ops)` can accelerate
/// POSIX `~` (since PostgreSQL 9.3) whenever the pattern yields extractable
/// trigrams — `urn:ngsi-ld:.*` does, an anchored `^A.*` does not — and it
/// intersects those trigrams with the attribute key, so a pattern common
/// across attributes yields candidates only from the one probed. When no
/// trigram is extractable the probe is still narrowed by the
/// `attribute_name_key` prefix of `idx_ca_value_string` before any regex is
/// evaluated. `idx_ca_value_long_text_trgm` serves the overflow arm the same
/// way.
#[must_use]
pub(crate) fn pattern_value(pred: &Predicate, pattern: &Pattern, polarity: PatternPolarity, row: InstanceRow) -> Condition {
    let strings = row.string_columns().iter().fold(Condition::any(), |value, column| {
        value.add(pattern_predicate(row.value(*column), polarity, SeaExpr::val(pattern.portable().as_str())))
    });
    strings.add(language_term(pred, &LanguageMap::of_instance(row), LanguageSelection::AnyLanguage))
}
