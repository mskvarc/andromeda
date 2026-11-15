//! The target value of a compound value path (`address[city]`).

use crate::{
    q::{
        family_target::{family_target, first_family_value, held_target},
        geometry_member::geometry_member,
        target_value::{ResolvedTarget, json_member},
    },
    queried_entity::{
        attribute_reading::{QueriedInstance, QueriedValueNode},
        value_item::ValueItem,
    },
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::filter::{
    reading::family_reading::{MemberSource, MultiValueReading, TargetOrigin, family_reading},
    trailing_path::MemberPath,
};

/// Resolve a compound value path (`address[city]`) at one attribute or
/// sub-attribute instance the axis reaches.
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
///
/// A `GeoProperty`'s value is its geometry, whose members are read from the
/// `GeoJSON` object it is, by the names the client wrote ([`MemberSource`]).
pub(crate) fn resolve_compound_member<'a, I: QueriedInstance<'a>>(instance: I, members: &MemberPath) -> Option<ResolvedTarget<'a>> {
    match family_reading(instance.family()).members {
        MemberSource::Geometry => geometry_member_target(instance, &members.written),
        MemberSource::Value => {
            resolve_member_hops(first_family_value(instance), &members.addressed).or_else(|| json_member(&family_target(instance)?, &members.written))
        }
    }
}

/// The member of the geometry `instance` holds that `written` names, read
/// from the `GeoJSON` object the geometry is.
fn geometry_member_target<'a, I: QueriedInstance<'a>>(instance: I, written: &[AttributeName]) -> Option<ResolvedTarget<'a>> {
    match first_family_value(instance)? {
        ValueItem::Geometry(form) => geometry_member(form, written),
        ValueItem::Literal(_) | ValueItem::Json(_) | ValueItem::Reference(_) | ValueItem::Node(_) | ValueItem::Opaque => None,
    }
}

/// Walk the members of a structured value down to the target the last member
/// holds.
///
/// The member's values are the target value as the values of a Property are
/// ([`held_target`]): a literal is its value and a structured value an
/// Object, one value is a scalar target and several an array (clause 4.9),
/// and a member holding the empty array holds the empty array, a defined
/// value.
fn resolve_member_hops<'a, N: QueriedValueNode<'a>>(start: Option<ValueItem<'a, N>>, value_path: &[AttributeName]) -> Option<ResolvedTarget<'a>> {
    Some(ResolvedTarget {
        value: held_target(walk_member_hops(start, value_path)?),
        origin: TargetOrigin::AttributeValue,
        reading: MultiValueReading::WholeArray,
    })
}

/// The values of the member a structured value's member path leads to, or
/// `None` when a step finds no member to descend into or the last member is
/// absent.
///
/// A member expression accesses a property of an object and an array has
/// none, so every step before the last descends only into a member holding
/// exactly one value, a node.
fn walk_member_hops<'a, N: QueriedValueNode<'a>>(
    start: Option<ValueItem<'a, N>>,
    value_path: &[AttributeName],
) -> Option<impl Iterator<Item = ValueItem<'a, N>>> {
    let (last, steps) = value_path.split_last()?;
    let node = steps.iter().try_fold(start?, |current, member| match current {
        ValueItem::Node(node) => sole_value(node.member_values(member)?),
        ValueItem::Literal(_) | ValueItem::Json(_) | ValueItem::Reference(_) | ValueItem::Geometry(_) | ValueItem::Opaque => None,
    })?;
    match node {
        ValueItem::Node(node) => node.member_values(last),
        ValueItem::Literal(_) | ValueItem::Json(_) | ValueItem::Reference(_) | ValueItem::Geometry(_) | ValueItem::Opaque => None,
    }
}

/// The one value `values` holds, or `None` when it holds none or several.
fn sole_value<T>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let first = values.next()?;
    values.next().is_none().then_some(first)
}
