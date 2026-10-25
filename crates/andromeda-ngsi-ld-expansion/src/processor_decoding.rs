//! Reading the JSON-LD processor's expanded document as the written document.
//!
//! When the typed walk declines a request, the processor expands it, and its
//! typed output (objects, nodes, value objects with each number's exact
//! text, list objects and JSON literals) is read straight into the expanded
//! tree the walk would have produced, then through the same assembly. Nothing
//! passes through a JSON tree on the way; a JSON literal is printed from the
//! processor's own JSON and admitted as the model carries it. The reading
//! recurses once per level of the document, which a nested value or
//! sub-attribute deepens, so every level runs on sufficient stack.

use crate::{
    assembly::{attribute_fragment, entity_document, member_value::malformed, temporal_document::temporal_document},
    assembly_error::AssemblyError,
    attribute_fragment::AttributeFragment,
    compact_walk::literal::direction_of,
    expanded_tree::{ExpandedItem, ExpandedNode, Scalar, ValueObject},
    term_scope::id_text,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_native_stack::on_sufficient_stack;
use andromeda_ngsi_ld_entity::{document::EntityDocument, member_shape::MemberShape, refusal::EntityRefusal};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::{entity_system_times::EntitySystemTimes, temporal::temporal_document::TemporalDocument};
use iri_rs::IriBuf;
use jsonld::{ExpandedDocument, IndexedObject, Node, NodeParts, Object, Value as ObjectValue, object::Literal};
use rdfx::BlankIdBuf;

/// Read the processor's expansion of an entity document.
///
/// # Errors
///
/// Returns [`EntityRefusal::Malformed`] for a document that expanded to more
/// than one top-level object or to something other than a node, and the
/// assembly's failure on the node.
pub fn decode_entity(document: ExpandedDocument<IriBuf, BlankIdBuf>) -> Result<EntityDocument, AssemblyError> {
    entity_document(top_level_node(document)?)
}

/// Read the processor's expansion of a Temporal Evolution, `system_times`
/// being the entity's own clause 4.8 times it was written with.
///
/// # Errors
///
/// As [`decode_entity`].
pub fn decode_temporal(document: ExpandedDocument<IriBuf, BlankIdBuf>, system_times: EntitySystemTimes) -> Result<TemporalDocument, AssemblyError> {
    temporal_document(top_level_node(document)?, system_times)
}

/// Read the processor's expansion of an attribute fragment written as
/// `{"<name>": <fragment>}`.
///
/// # Errors
///
/// Returns [`EntityRefusal::Malformed`] when the expansion holds no attribute
/// member, or holds more than one, and the assembly's failure on the instance.
pub fn decode_attribute_fragment(attribute: &AttributeName, document: ExpandedDocument<IriBuf, BlankIdBuf>) -> Result<AttributeFragment, AssemblyError> {
    let node = top_level_node(document)?.ok_or_else(|| malformed(attribute, MemberShape::Attribute))?;
    let mut members = node.members.into_iter();
    let (Some((name, items)), None) = (members.next(), members.next()) else {
        return Err(malformed(attribute, MemberShape::Attribute).into());
    };
    attribute_fragment(&name, items)
}

/// The one node a document expanded to, or none.
fn top_level_node(document: ExpandedDocument<IriBuf, BlankIdBuf>) -> Result<Option<ExpandedNode>, EntityRefusal> {
    let root = || EntityRefusal::Malformed {
        attribute: None,
        member: MemberShape::Root,
    };
    let mut objects = document.into_objects().into_iter();
    match (objects.next(), objects.next()) {
        (None, _) => Ok(None),
        (Some(object), None) => match item(object)? {
            ExpandedItem::Node(node) => Ok(Some(node)),
            ExpandedItem::Value(_) | ExpandedItem::Json(_) | ExpandedItem::List(_) | ExpandedItem::Geometry(_) => Err(root()),
        },
        (Some(_), Some(_)) => Err(root()),
    }
}

/// Read one expanded object, refusing a shape no NGSI-LD document holds (a
/// graph, an `@included` or `@reverse` member, an `@index`, a `null` literal).
fn item(object: IndexedObject<IriBuf, BlankIdBuf>) -> Result<ExpandedItem, EntityRefusal> {
    on_sufficient_stack(|| object_item(object))
}

/// Read one expanded object on the stack the caller left.
fn object_item(object: IndexedObject<IriBuf, BlankIdBuf>) -> Result<ExpandedItem, EntityRefusal> {
    let (object, index) = object.into_parts();
    if index.is_some() {
        return Err(unsupported());
    }
    match object {
        Object::Value(value) => value_item(value),
        Object::Node(node) => node_item(*node).map(ExpandedItem::Node),
        Object::List(list) => list.into_iter().map(item).collect::<Result<Vec<_>, EntityRefusal>>().map(ExpandedItem::List),
    }
}

/// Read one value object.
fn value_item(value: ObjectValue<IriBuf>) -> Result<ExpandedItem, EntityRefusal> {
    match value {
        ObjectValue::Literal(literal, datatype) => Ok(ExpandedItem::Value(ValueObject {
            scalar: scalar(literal)?,
            datatype: datatype.map(|datatype| datatype.as_str().to_owned()),
            language: None,
            direction: None,
        })),
        ObjectValue::LangString(text) => {
            let (text, language, direction) = text.into_parts();
            Ok(ExpandedItem::Value(ValueObject {
                scalar: Scalar::String(text.as_str().to_owned()),
                datatype: None,
                language: language.map(|language| language.as_str().to_owned()),
                direction: direction.map(direction_of),
            }))
        }
        ObjectValue::Json(json) => JsonLiteral::from_processor_json(&json).map(ExpandedItem::Json).map_err(|_| unsupported()),
    }
}

/// The scalar of a literal, numbers as their exact text.
fn scalar(literal: Literal) -> Result<Scalar, EntityRefusal> {
    match literal {
        Literal::Null => Err(unsupported()),
        Literal::Boolean(flag) => Ok(Scalar::Boolean(flag)),
        Literal::Number(number) => Ok(Scalar::Number(number.as_str().to_owned())),
        Literal::String(text) => Ok(Scalar::String(text.as_str().to_owned())),
    }
}

/// Read one node object.
fn node_item(node: Node<IriBuf, BlankIdBuf>) -> Result<ExpandedNode, EntityRefusal> {
    let NodeParts {
        id,
        types,
        graph,
        included,
        properties,
        reverse_properties,
    } = node.into_parts();
    if graph.is_some() || included.is_some() || reverse_properties.is_some() {
        return Err(unsupported());
    }
    let mut expanded = ExpandedNode {
        id: id.as_ref().map(id_text),
        types: types.unwrap_or_default().iter().map(id_text).collect(),
        members: Vec::new(),
    };
    for (property, objects) in properties {
        let values = objects.into_iter().map(item).collect::<Result<Vec<_>, EntityRefusal>>()?;
        expanded.add_member(id_text(&property), values);
    }
    Ok(expanded)
}

/// The refusal of an expanded shape no NGSI-LD document holds.
const fn unsupported() -> EntityRefusal {
    EntityRefusal::Malformed {
        attribute: None,
        member: MemberShape::NestedValue,
    }
}
