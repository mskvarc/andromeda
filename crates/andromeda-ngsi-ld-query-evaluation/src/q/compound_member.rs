//! The target value of a compound value path (`address[city]`).

use crate::{
    q::{
        axis_position::AxisPosition,
        family_target::{TerminalKeyword, held_values_target},
        target_value::{MultiValueReading, ResolvedTarget, TargetOrigin, json_member},
    },
    queried_entity::{
        attribute_reading::{QueriedInstance, QueriedValueNode},
        value_item::ValueItem,
    },
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::filter::trailing_path::MemberPath;

/// Resolve a compound value path (`address[city]`) at one position of the
/// axis.
///
/// A compound value reaches this evaluator in either of two genuinely different
/// shapes, and both are addressed by the same `q` path:
///
/// * A structured Property value is a node whose members are keyed by expanded
///   IRI, so `city` is a member *hop*: the first value, then the member's
///   first value, down to a literal. This is the shape a notification payload and a stored entity carry.
/// * A `JsonProperty`'s value, and a Property value held as an opaque JSON
///   object, are raw JSON, where `city` is an ordinary object key. The NGSI-LD
///   data model puts no constraint on those keys, so they are read verbatim.
///
/// The member hop is tried first, addressed by the term-expanded spelling
/// ([`MemberPath::addressed`]): it is the shape term expansion produced the
/// path for, and a raw JSON object cannot carry a member whose key is the
/// expanded IRI without the client having written that IRI. The raw-JSON
/// fallback then retries with the pre-expansion spelling
/// ([`MemberPath::written`]), because raw JSON is never touched by JSON-LD
/// expansion: its keys are exactly what the client wrote, not what the request
/// `@context` expands them to.
pub(crate) fn resolve_compound_member<'a, I: QueriedInstance<'a>>(position: AxisPosition<'a, I>, members: &MemberPath) -> Option<ResolvedTarget<'a>> {
    if let Some(target) = resolve_member_hops(position.first_family_value(), &members.addressed) {
        return Some(target);
    }
    json_member(&position.terminal()?, &members.written)
}

/// Walk the members of a structured value down to the target the last member
/// holds.
///
/// The member's literals are the target value as the values of a Property
/// are: one is a scalar target, several an array (clause 4.9), and a member
/// holding no literal holds no target value.
fn resolve_member_hops<'a, N: QueriedValueNode<'a>>(start: Option<ValueItem<'a, N>>, value_path: &[AttributeName]) -> Option<ResolvedTarget<'a>> {
    Some(ResolvedTarget {
        value: held_values_target(walk_member_hops(start, value_path)?.filter_map(|item| TerminalKeyword::Literal.read(item)))?,
        origin: TargetOrigin::AttributeValue,
        reading: MultiValueReading::WholeArray,
    })
}

/// The values of the member a structured value's member path leads to, or
/// `None` when a step finds no member to descend into.
///
/// A member expression accesses a property of an object and an array has
/// none, so every step before the last descends only into a member holding
/// exactly one value, a node.
pub(crate) fn walk_member_hops<'a, N: QueriedValueNode<'a>>(
    start: Option<ValueItem<'a, N>>,
    value_path: &[AttributeName],
) -> Option<impl Iterator<Item = ValueItem<'a, N>>> {
    let (last, steps) = value_path.split_last()?;
    let node = steps.iter().try_fold(start?, |current, member| match current {
        ValueItem::Node(node) => sole_value(node.member_values(member)),
        ValueItem::Literal(_) | ValueItem::Json(_) | ValueItem::Reference(_) | ValueItem::Opaque => None,
    })?;
    match node {
        ValueItem::Node(node) => {
            let mut values = node.member_values(last).peekable();
            values.peek().is_some().then_some(values)
        }
        ValueItem::Literal(_) | ValueItem::Json(_) | ValueItem::Reference(_) | ValueItem::Opaque => None,
    }
}

/// The one value `values` holds, or `None` when it holds none or several.
fn sole_value<T>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let first = values.next()?;
    values.next().is_none().then_some(first)
}
