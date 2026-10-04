//! The pattern Query Terms (`~=`/`!~=`, clause 4.9 `patternOp` and
//! `notPatternOp`) compiled against the stored instances of a local attribute
//! path.

use crate::{
    dictionary::query_names::QueryNames,
    operator::pattern as pattern_predicate,
    q_predicate::{
        fallback::fallback_pattern,
        instance_scan::InstanceScan,
        instance_test::{CompiledTerm, InstanceTest},
        language_map::{LanguageMap, LanguageSelection},
        language_term::language_term,
        local_target::LocalTarget,
        projection::{unsatisfiable, unsatisfiable_term},
    },
};
use andromeda_ngsi_ld_query::{
    filter::{
        path::AttrPath,
        predicate::{PatternPolarity, Predicate},
    },
    pattern::Pattern,
};
use sea_query::{Condition, Expr as SeaExpr};

/// Compile `~=` / `!~=`, routing it by what its path addresses.
///
/// Both polarities compile *inside* the `EXISTS`, so both require the attribute
/// to be present. Clause 4.9 is explicit that the entity "shall contain the
/// target element" for `notPatternOp`; an earlier compiler wrapped the whole
/// existence test in `NOT (...)`, which matched entities that simply lacked the
/// attribute.
///
/// A regex is not a containment test, so a trailing member's two readings are
/// both compiled string by string: the target language of a `LanguageProperty`
/// against the projection, the compound value member against the payload.
pub(crate) fn compile_pattern(
    pred: &Predicate,
    path: &AttrPath,
    target: LocalTarget<'_>,
    pattern: &Pattern,
    polarity: PatternPolarity,
    scan: &InstanceScan<'_>,
    names: &QueryNames,
) -> CompiledTerm {
    let fallback = || scan.document_condition(&path.attr, names, |document| fallback_pattern(path, pattern, polarity, &document));
    match target {
        LocalTarget::OwnValue => match names.attributes.get(&path.attr) {
            Some(id) => CompiledTerm::Instances(InstanceTest {
                attribute: id,
                value: own_value_pattern(pred, pattern, polarity, scan),
            }),
            None => unsatisfiable_term(),
        },
        LocalTarget::TrailingMember(member) => {
            let tagged = match names.attributes.get(&path.attr) {
                Some(id) => scan.exists(
                    id,
                    Condition::all().add(language_term(pred, &LanguageMap::of_instance(scan), LanguageSelection::TargetLanguage(member))),
                ),
                None => unsatisfiable(),
            };
            CompiledTerm::Standalone(Condition::any().add(tagged).add(fallback()))
        }
        LocalTarget::PayloadMember | LocalTarget::SubAttribute(_) => CompiledTerm::Standalone(fallback()),
    }
}

/// A pattern over the attribute's own value.
///
/// Only the two string columns are tested, `value_string` and the overflow
/// `value_long_text`, as one disjunction: a row's value sits in exactly one of
/// them, so the other arm is NULL and the disjunction reads the one that holds
/// it. Clause 4.9 requires a Relationship operand to non-match every operator
/// other than equality and inequality, and the pattern operators are defined
/// over String targets — so a Relationship's object, which is the whole content
/// of `value_uri`, is never a pattern target. Probing it made
/// `owner~="urn:Person:.*"` select entities the standard requires it to reject.
///
/// A `LanguageProperty` row holds its strings in `value_object` instead, and
/// clause 4.9 makes each of them a String target value when no target language
/// is given, so its `languageMap` is read beside the string columns: some
/// string has to match `~=`, and none may match `!~=`.
///
/// The attribute-aware trigram index `idx_cav_value_string_trgm` on
/// `(attribute_name_key int4_ops, value_string gin_trgm_ops)` can accelerate
/// POSIX `~` (since PostgreSQL 9.3) whenever the pattern yields extractable
/// trigrams — `urn:ngsi-ld:.*` does, an anchored `^A.*` does not — and it
/// intersects those trigrams with the attribute key, so a pattern common
/// across attributes yields candidates only from the one probed. When no
/// trigram is extractable the probe is still narrowed by the
/// `attribute_name_key` prefix of `idx_cav_value_string` before any regex is
/// evaluated. `idx_cav_value_long_text_trgm` serves the overflow arm the same
/// way.
fn own_value_pattern(pred: &Predicate, pattern: &Pattern, polarity: PatternPolarity, scan: &InstanceScan<'_>) -> Condition {
    let strings = scan.string_columns().iter().fold(Condition::any(), |value, column| {
        value.add(pattern_predicate(scan.value(*column), polarity, SeaExpr::val(pattern.portable().as_str())))
    });
    strings.add(language_term(pred, &LanguageMap::of_instance(scan), LanguageSelection::AnyLanguage))
}
