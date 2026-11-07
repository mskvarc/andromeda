//! Expanding one element: a scalar, an array or an object (JSON-LD 1.1
//! expansion algorithm, steps 1 to 13).

use crate::{
    compact_walk::{literal::expand_scalar, object::expand_object},
    decline::Decline,
    expanded_tree::ExpandedItem,
    term_scope::TermScope,
};
use andromeda_ngsi_ld_representation::compact_node::{CompactNode, NodeShape};
use jsonld::ContainerKind;

/// What one element expands to.
#[derive(Debug, PartialEq)]
pub(crate) enum Expanded {
    /// Nothing: a `null`, a dropped node or a free-floating value.
    Null,
    /// One item.
    One(ExpandedItem),
    /// The items of an array, in order.
    Many(Vec<ExpandedItem>),
}

impl Expanded {
    /// The items, in order.
    #[must_use]
    pub(crate) fn into_items(self) -> Vec<ExpandedItem> {
        match self {
            Expanded::Null => Vec::new(),
            Expanded::One(item) => vec![item],
            Expanded::Many(items) => items,
        }
    }
}

/// Expand `element`, the value of `active_property` (`None` at the top of the
/// document).
///
/// # Errors
///
/// Declines whatever the walk does not model.
pub(crate) fn expand_element<'n, N: CompactNode<'n>>(scope: &TermScope<'_>, active_property: Option<&str>, element: N) -> Result<Expanded, Decline> {
    match element.shape() {
        NodeShape::Null => Ok(Expanded::Null),
        NodeShape::Array(items) => {
            let mut result = Vec::with_capacity(scope.value_capacity(items.len()));
            for item in items {
                result.extend(expand_element(scope, active_property, item)?.into_items());
            }
            list_or_array(scope, active_property, result)
        }
        NodeShape::Object(members) => expand_object(&scope.object_scope(active_property)?, active_property, members),
        // A geometry is held typed only where the typed walk expands it as
        // the geometry it is; met as JSON, it is not the walk's to expand.
        NodeShape::Geometry(_) => Err(Decline::CompactSerialization),
        NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) => {
            let Some(property) = active_property else {
                return Ok(Expanded::Null);
            };
            // A scalar is expanded under a property-scoped context processed
            // without overriding protected terms, unlike an object: a case the
            // precomputed scoped contexts do not cover.
            if scope.has_scoped_context(property) {
                return Err(Decline::ScopedScalar);
            }
            scope.count_value()?;
            expand_scalar(scope, property, element).map(Expanded::One)
        }
    }
}

/// The items of an array, wrapped in one list object when the property's
/// container is `@list` (expansion of an array, JSON-LD 1.1 step 5).
///
/// # Errors
///
/// Declines a list object past the write's value limit.
pub(crate) fn list_or_array(scope: &TermScope<'_>, active_property: Option<&str>, items: Vec<ExpandedItem>) -> Result<Expanded, Decline> {
    let is_list = active_property
        .and_then(|property| scope.definition(property))
        .is_some_and(|definition| definition.container().contains(ContainerKind::List));
    if is_list {
        scope.count_value()?;
        Ok(Expanded::One(ExpandedItem::List(items)))
    } else {
        Ok(Expanded::Many(items))
    }
}
