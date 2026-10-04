//! A Property value is any JSON object (clauses 3.1 and 4.6.3): its keys, the
//! `@id` the core context's `id` alias gives it, and the `@type` its `type`
//! alias gives it are kept as expansion makes them, on both expansion paths,
//! though none of them is an NGSI-LD name (clause 4.6.2 restricts attribute
//! names only).

use crate::{
    expansion_fixture::{engine, entity},
    normalization::Normalization,
};
use andromeda_ngsi_ld_entity::{
    document::{
        EntityDocument,
        instance_document::InstanceDocument,
        written_value::{ValueSlot, WrittenValue},
    },
    expanded_value::{ExpandedValue, ValueNode},
    expanded_value_name::ExpandedValueName,
    geo_value::GeoValue,
    node_id::NodeId,
    value_mode::{Fragment, FragmentMember},
};
use serde_json::{Value, json};

/// The core context's default vocabulary (clause 5.5.7).
const DEFAULT: &str = "https://uri.etsi.org/ngsi-ld/default-context/";

/// The entity both expansions agree on, its one attribute `p` written as
/// `attribute`.
async fn agreed(attribute: Value) -> EntityDocument {
    entity(
        &engine(),
        json!({"id": "urn:ngsi-ld:Shop:1", "type": "Shop", "p": attribute}),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed()
}

/// The one instance of `p`.
fn instance(document: &EntityDocument) -> &InstanceDocument {
    let member = document.attributes().get(format!("{DEFAULT}p").as_str()).expect("p is written");
    let [instance] = member.instances() else {
        panic!("p carries one instance");
    };
    instance
}

/// The structured value of `p`.
fn node(document: &EntityDocument) -> &ValueNode<Fragment> {
    let ValueSlot::Set(WrittenValue::Value(values)) = instance(document).value() else {
        panic!("a Property value");
    };
    let [ExpandedValue::Node(node)] = values.as_slice() else {
        panic!("one node, got {values:?}");
    };
    node
}

/// The member names of a node, in order.
fn names(node: &ValueNode<Fragment>) -> Vec<&str> {
    node.members().keys().map(ExpandedValueName::as_str).collect()
}

#[tokio::test]
async fn a_key_that_is_no_attribute_name_is_kept_as_its_expansion() {
    let document = agreed(json!({"type": "Property", "value": {"opening hours": "9-5"}})).await;
    pretty_assertions::assert_eq!(names(node(&document)), vec![format!("{DEFAULT}opening hours")]);
}

#[tokio::test]
async fn such_a_key_is_kept_nested_and_inside_an_array() {
    let document = agreed(json!({"type": "Property", "value": {"outer": {"opening hours": "9-5"}, "list": [{"a b": 1}, {"c d": 2}]}})).await;
    let value = node(&document);
    let Some(FragmentMember::Values(outer)) = value.members().get(format!("{DEFAULT}outer").as_str()) else {
        panic!("outer is a member");
    };
    let [ExpandedValue::Node(outer)] = outer.as_slice() else {
        panic!("outer is a node");
    };
    pretty_assertions::assert_eq!(names(outer), vec![format!("{DEFAULT}opening hours")]);
    let Some(FragmentMember::Values(list)) = value.members().get(format!("{DEFAULT}list").as_str()) else {
        panic!("list is a member");
    };
    pretty_assertions::assert_eq!(list.len(), 2);
}

/// The core context aliases `id` to `@id`; an NGSI-LD document has no base
/// IRI, so a relative reference stays as written.
#[tokio::test]
async fn a_relative_id_is_kept_verbatim() {
    let document = agreed(json!({"type": "Property", "value": {"id": "abc", "name": "x"}})).await;
    let value = node(&document);
    pretty_assertions::assert_eq!(value.id(), Some(&NodeId::from("abc")));
    assert!(matches!(value.id(), Some(NodeId::Verbatim(_))));
    pretty_assertions::assert_eq!(names(value), vec![format!("{DEFAULT}name")]);
}

/// The core context aliases `type` to `@type`, which expands vocabulary
/// relative whether or not the result is a well-formed IRI.
#[tokio::test]
async fn a_type_that_is_no_name_is_kept_as_its_expansion() {
    let document = agreed(json!({"type": "Property", "value": {"type": "Postal Address", "street": "x"}})).await;
    let types: Vec<&str> = node(&document).types().iter().map(ExpandedValueName::as_str).collect();
    pretty_assertions::assert_eq!(types, vec![format!("{DEFAULT}Postal Address")]);
}

/// IETF RFC 7946 section 6.1 lets a geometry carry any foreign member.
#[tokio::test]
async fn a_geometry_foreign_member_that_is_no_attribute_name_is_kept() {
    let document = agreed(json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 2], "plot name": "7"}})).await;
    let ValueSlot::Set(WrittenValue::Geometry(GeoValue::Geometry(geometry))) = instance(&document).value() else {
        panic!("a geometry");
    };
    let foreign = geometry.members().expect("the members are kept").foreign();
    pretty_assertions::assert_eq!(
        foreign.keys().map(ExpandedValueName::as_str).collect::<Vec<_>>(),
        vec![format!("{DEFAULT}plot name")]
    );
}
