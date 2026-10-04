//! The ordering Query Terms (`>`/`>=`/`<`/`<=`, clause 4.9) compiled against
//! the stored instances of a local attribute path.

use crate::{
    attribute_name_key::AttributeNameKey,
    dictionary::query_names::QueryNames,
    q_predicate::{
        fallback::fallback_ordering,
        instance_scan::InstanceScan,
        instance_test::{CompiledTerm, InstanceTest},
        language_map::{LanguageMap, LanguageSelection},
        language_term::{StringComparability, language_term, string_comparability},
        local_target::LocalTarget,
        projection::{unsatisfiable, unsatisfiable_term},
        typed_column::typed_column_condition,
    },
};
use andromeda_ngsi_ld_query::filter::{
    path::AttrPath,
    predicate::{OrderingOp, Predicate},
    scalar::Scalar,
};
use sea_query::Condition;

/// Compile an ordering Query Term, routing it by what its path addresses.
///
/// An ordering comparison is not a containment test, so a trailing member's
/// two readings are both compiled string by string: the target language of a
/// `LanguageProperty` against the projection, the compound value member against
/// the payload, disjoined exactly as an equality term disjoins them.
pub(crate) fn compile_ordering(
    pred: &Predicate,
    path: &AttrPath,
    target: LocalTarget<'_>,
    op: OrderingOp,
    rhs: &Scalar,
    scan: &InstanceScan<'_>,
    names: &QueryNames,
) -> CompiledTerm {
    let fallback = || scan.document_condition(&path.attr, names, |document| fallback_ordering(path, op, rhs, &document));
    match target {
        LocalTarget::OwnValue => match names.attributes.get(&path.attr) {
            Some(id) => CompiledTerm::Instances(ordering_on_projection(pred, id, op, rhs, scan)),
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

/// Compare the attribute's own value: the typed column holding the operand's
/// kind, and — for an operand a string can compare with — every string of a
/// `languageMap`, each of which clause 4.9 makes a target value when no target
/// language is given.
///
/// A number or a boolean is compared against its typed column alone, which no
/// string can satisfy anyway, so its probe stays on that column's index.
fn ordering_on_projection(pred: &Predicate, id: AttributeNameKey, op: OrderingOp, rhs: &Scalar, scan: &InstanceScan<'_>) -> InstanceTest {
    let typed = typed_column_condition(rhs, op.as_cmp_op(), scan);
    let value = match string_comparability(rhs) {
        StringComparability::Comparable => {
            Condition::any()
                .add(typed)
                .add(language_term(pred, &LanguageMap::of_instance(scan), LanguageSelection::AnyLanguage))
        }
        StringComparability::Incomparable => Condition::all().add(typed),
    };
    InstanceTest { attribute: id, value }
}
