//! Expanding the value of one member of a node object (JSON-LD 1.1 expansion
//! algorithm, steps 13.6 to 13.14).

use crate::{
    compact_walk::{
        element::{Expanded, expand_element},
        literal::direction_of,
        object::force_array,
        value_object::json_literal,
    },
    decline::Decline,
    expanded_tree::{ExpandedItem, Scalar, ValueObject},
    term_scope::TermScope,
};
use andromeda_ngsi_ld_representation::compact_node::{CompactNode, NodeMembers, NodeShape};
use jsonld::{ContainerKind, Nullable, Term, Type, syntax::Keyword};

/// Expand the value written under the member `key`, or `None` when the value
/// expands to nothing and the member is not created at all.
///
/// # Errors
///
/// Declines a reverse property, a container the walk does not model, and
/// whatever expanding the value declines.
pub fn expand_member<'n, N: CompactNode<'n>>(scope: &TermScope<'_>, key: &str, value: N) -> Result<Option<Vec<ExpandedItem>>, Decline> {
    let definition = scope.definition(key);
    if definition.is_some_and(|definition| definition.reverse_property()) {
        return Err(Decline::ReverseProperty);
    }
    let container = definition.map(|definition| definition.container());
    let has = |kind: ContainerKind| container.is_some_and(|container| container.contains(kind));
    if has(ContainerKind::Index) || has(ContainerKind::Id) || has(ContainerKind::Type) || has(ContainerKind::Graph) {
        return Err(Decline::UnsupportedContainer);
    }
    // A term typed `@json` takes its value verbatim (step 13.7).
    if definition.and_then(|definition| definition.typ()) == Some(&Type::Json) {
        return Ok(Some(vec![ExpandedItem::Json(json_literal(value)?)]));
    }
    let expanded = match value.shape() {
        NodeShape::Object(map) if has(ContainerKind::Language) => Expanded::Many(language_map(scope, key, map)?),
        NodeShape::Null
        | NodeShape::Boolean(_)
        | NodeShape::Number(_)
        | NodeShape::String(_)
        | NodeShape::Array(_)
        | NodeShape::Object(_)
        | NodeShape::Geometry(_) => expand_element(scope, Some(key), value)?,
    };
    Ok(wrap_list(has(ContainerKind::List), expanded))
}

/// The values of a member whose items were expanded already, as an array
/// written under `key` expands (JSON-LD 1.1 expansion of an array, step 5,
/// then step 13.13).
///
/// # Errors
///
/// Declines a definition of `key` under which the items would not have
/// expanded as they were: a reverse property, a `@json` or `@language` term,
/// or any container but `@list` and `@set`.
pub fn expanded_member(scope: &TermScope<'_>, key: &str, items: Vec<ExpandedItem>, written: Written) -> Result<Vec<ExpandedItem>, Decline> {
    let definition = scope.definition(key);
    if definition.is_some_and(|definition| definition.reverse_property()) {
        return Err(Decline::ReverseProperty);
    }
    if definition.and_then(|definition| definition.typ()) == Some(&Type::Json) {
        return Err(Decline::UnsupportedContainer);
    }
    let container = definition.map(|definition| definition.container());
    let has = |kind: ContainerKind| container.is_some_and(|container| container.contains(kind));
    if [
        ContainerKind::Index,
        ContainerKind::Id,
        ContainerKind::Type,
        ContainerKind::Graph,
        ContainerKind::Language,
    ]
    .into_iter()
    .any(has)
    {
        return Err(Decline::UnsupportedContainer);
    }
    let expanded = match (written, has(ContainerKind::List)) {
        (Written::Array, true) => Expanded::One(ExpandedItem::List(items)),
        (Written::Array | Written::Single, false) | (Written::Single, true) => Expanded::Many(items),
    };
    Ok(wrap_list(has(ContainerKind::List), expanded).unwrap_or_default())
}

/// How a member whose items were expanded already was written.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Written {
    /// As one value.
    Single,
    /// As an array of values.
    Array,
}

/// A member's expanded value, as a list object when its container is `@list`
/// and it is not one already (step 13.13); `None` for a value that expanded to
/// nothing.
fn wrap_list(list_container: bool, expanded: Expanded) -> Option<Vec<ExpandedItem>> {
    match expanded {
        Expanded::Null => None,
        Expanded::One(ExpandedItem::List(items)) => Some(vec![ExpandedItem::List(items)]),
        expanded @ (Expanded::One(_) | Expanded::Many(_)) if list_container => Some(vec![ExpandedItem::List(expanded.into_items())]),
        expanded @ (Expanded::One(_) | Expanded::Many(_)) => Some(expanded.into_items()),
    }
}

/// Expand a language map: one value object per string, tagged with the map
/// key unless the key is `@none` (step 13.7.4).
fn language_map<'n, N: CompactNode<'n>>(scope: &TermScope<'_>, key: &str, map: NodeMembers<'n, N>) -> Result<Vec<ExpandedItem>, Decline> {
    let direction = match scope.definition(key).and_then(|definition| definition.direction()) {
        Some(Nullable::Some(direction)) => Some(direction),
        Some(Nullable::Null) => None,
        None => scope.default_direction(),
    };
    let mut values = Vec::new();
    for (tag, texts) in map {
        let language = match scope.expand_key(&tag)?.as_ref() {
            Term::Keyword(Keyword::None) => None,
            Term::Null | Term::Id(_) | Term::Keyword(_) => Some(tag.into_owned()),
        };
        for text in force_array(texts) {
            match text.shape() {
                NodeShape::Null => {}
                NodeShape::String(text) => values.push(ExpandedItem::Value(ValueObject {
                    scalar: Scalar::String(text.into_owned()),
                    datatype: None,
                    language: language.clone(), // clone: every string of the entry carries the entry's language
                    direction: direction.map(direction_of),
                })),
                NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::Array(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => {
                    return Err(Decline::MalformedJsonLd);
                }
            }
        }
    }
    Ok(values)
}
