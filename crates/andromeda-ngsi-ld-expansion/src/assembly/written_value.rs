//! Reading the value member of one written attribute instance.
//!
//! Each family carries its value in one member (clause 4.5): the member is
//! what decides how the expanded values read, except that a `GeoProperty`
//! reads `value` as a geometry (clause 4.7) where every other family reads it
//! as a Property value.

use crate::{
    assembly::{
        expanded_value::{expanded_values, literal},
        geometry_node::{geometry_of, geometry_value},
        member_value::{malformed, null_beside_data, reference_id},
        null_sentinel::{is_deletion, is_list_object_null, is_null},
    },
    assembly_error::AssemblyError,
    expanded_tree::{ExpandedItem, ExpandedNode, Scalar, ValueObject},
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_entity::{
    document::{
        value_carrier::ValueCarrier,
        written_value::{ValueSlot, WrittenValue},
    },
    geo_value::{EncodedGeometry, GeoValue},
    language_entries::{LanguageEntries, LanguageEntry},
    list_object::ListObject,
    literal::LiteralValue,
    member_shape::MemberShape,
    non_empty::NonEmpty,
    refusal::EntityRefusal,
    value_mode::{Fragment, FragmentText},
};
use andromeda_ngsi_ld_geometry::geojson::GeometryError;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, reference::NgsiLdReference, uri::Uri};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use compact_str::CompactString;
use langtag::LangTagBuf;

/// Read the values of the value member `carrier` of an instance of `family`.
///
/// The member is present: expansion drops a member whose value is `null`
/// altogether, but keeps one written as an empty array with no item (JSON-LD
/// 1.1 expansion, step 13.14). No item is the empty array, which clause 3.1
/// makes a Property value like any other; every other member holds at least
/// one target, term, string or geometry, so no item there is malformed.
///
/// # Errors
///
/// Returns the refusal of a value that is not what its member holds, and
/// [`AssemblyError::UnspellableOrdinate`] for a geometry held as a Property
/// value whose ordinate has no JSON number.
pub fn value_slot(
    attribute: &AttributeName,
    family: Option<AttributeType>,
    carrier: ValueCarrier,
    items: Vec<ExpandedItem>,
) -> Result<ValueSlot, AssemblyError> {
    if is_deletion(carrier, &items) {
        return Ok(ValueSlot::Remove(carrier));
    }
    let shape = carrier_shape(carrier);
    let written = match carrier {
        ValueCarrier::Value if family == Some(AttributeType::GeoProperty) => WrittenValue::Geometry(geo_value(attribute, items)?),
        ValueCarrier::Value => WrittenValue::Value(expanded_values(attribute, items)?),
        ValueCarrier::Object => WrittenValue::Object(non_empty(attribute, shape, each(items, |item| object(attribute, &item))?)?),
        ValueCarrier::LanguageMap => WrittenValue::LanguageMap(LanguageEntries::new(
            non_empty(attribute, shape, each(items, |item| language_entry(attribute, item))?)?.into_vec(),
        )),
        ValueCarrier::Vocab => WrittenValue::Vocab(non_empty(attribute, shape, each(items, |item| vocab(attribute, &item))?)?),
        ValueCarrier::Json => WrittenValue::Json(non_empty(attribute, shape, each(items, |item| json(attribute, item))?)?),
        ValueCarrier::ValueList => WrittenValue::ValueList(expanded_values(attribute, single_list(attribute, shape, items)?)?),
        ValueCarrier::ObjectList => WrittenValue::ObjectList(each(single_list(attribute, shape, items)?, |item| list_object(attribute, &item))?),
    };
    Ok(ValueSlot::Set(written))
}

/// The member shape a refusal of `carrier` names.
const fn carrier_shape(carrier: ValueCarrier) -> MemberShape {
    match carrier {
        ValueCarrier::Value => MemberShape::Value,
        ValueCarrier::Object => MemberShape::Object,
        ValueCarrier::LanguageMap => MemberShape::LanguageMap,
        ValueCarrier::Vocab => MemberShape::Vocab,
        ValueCarrier::Json => MemberShape::Json,
        ValueCarrier::ValueList => MemberShape::ValueList,
        ValueCarrier::ObjectList => MemberShape::ObjectList,
    }
}

/// Read every item with `read`.
fn each<T>(items: Vec<ExpandedItem>, read: impl FnMut(ExpandedItem) -> Result<T, EntityRefusal>) -> Result<Vec<T>, EntityRefusal> {
    items.into_iter().map(read).collect()
}

/// Refuse an item that is the NGSI-LD Null: a member that is no deletion
/// encoding holds data only.
fn data_item(attribute: &AttributeName, item: &ExpandedItem) -> Result<(), EntityRefusal> {
    if is_null(item) { Err(null_beside_data(attribute)) } else { Ok(()) }
}

/// The values, refused when there is none: a member holding no target, term,
/// string or literal (clauses 4.5.3.2, 4.5.18.2, 4.5.20.2, 4.5.24.2).
fn non_empty<T>(attribute: &AttributeName, shape: MemberShape, values: Vec<T>) -> Result<NonEmpty<T>, EntityRefusal> {
    NonEmpty::try_from_vec(values).map_err(|_| malformed(attribute, shape))
}

/// The items of the one list object a list family's member holds.
fn single_list(attribute: &AttributeName, shape: MemberShape, items: Vec<ExpandedItem>) -> Result<Vec<ExpandedItem>, EntityRefusal> {
    let mut items = items.into_iter();
    match (items.next(), items.next()) {
        (Some(ExpandedItem::List(list)), None) => Ok(list),
        (Some(_) | None, _) => Err(malformed(attribute, shape)),
    }
}

/// The geometry of a `GeoProperty` (clause 4.7): the `GeoJSON` node, or the JSON
/// string of clause 4.7.2.
fn geo_value(attribute: &AttributeName, items: Vec<ExpandedItem>) -> Result<GeoValue, AssemblyError> {
    let refused = |source| EntityRefusal::GeoProperty {
        attribute: attribute.clone(), // clone: the refusal names the attribute the caller keeps
        source,
    };
    let mut items = items.into_iter();
    let (Some(item), None) = (items.next(), items.next()) else {
        return Err(malformed(attribute, MemberShape::Geometry).into());
    };
    match item {
        ExpandedItem::Geometry(geometry) => geometry_value(attribute, *geometry).map(GeoValue::Geometry),
        ExpandedItem::Node(node) => geometry_value(attribute, geometry_of(node).map_err(refused)?).map(GeoValue::Geometry),
        ExpandedItem::Value(value) => match literal(attribute, value)?.value() {
            LiteralValue::String(text) => Ok(EncodedGeometry::parse(text.to_string()).map(GeoValue::Encoded).map_err(refused)?),
            LiteralValue::Number(_) | LiteralValue::Boolean(_) => Err(refused(GeometryError::NotAnObject).into()),
        },
        ExpandedItem::Json(_) | ExpandedItem::List(_) => Err(refused(GeometryError::NotAnObject).into()),
    }
}

/// One `object` target: a node reference (the core context types `object`
/// `@id`).
fn object(attribute: &AttributeName, item: &ExpandedItem) -> Result<Uri, EntityRefusal> {
    data_item(attribute, item)?;
    match item {
        ExpandedItem::Node(node) => reference_id(node)
            .and_then(|id| id.parse::<Uri>().ok())
            .ok_or_else(|| malformed(attribute, MemberShape::Object)),
        ExpandedItem::Value(_) | ExpandedItem::Json(_) | ExpandedItem::List(_) | ExpandedItem::Geometry(_) => Err(malformed(attribute, MemberShape::Object)),
    }
}

/// One `vocab` entry: a node reference (the core context types `vocab`
/// `@vocab`).
fn vocab(attribute: &AttributeName, item: &ExpandedItem) -> Result<NgsiLdReference, EntityRefusal> {
    data_item(attribute, item)?;
    let ExpandedItem::Node(node) = item else {
        return Err(malformed(attribute, MemberShape::Vocab));
    };
    let id = reference_id(node).ok_or_else(|| malformed(attribute, MemberShape::Vocab))?;
    id.parse::<NgsiLdReference>().map_err(|source| EntityRefusal::Reference {
        attribute: attribute.clone(), // clone: the refusal names the attribute the caller keeps
        member: MemberShape::Vocab,
        source,
    })
}

/// One `json` value: a JSON literal (the core context types `json` `@json`).
fn json(attribute: &AttributeName, item: ExpandedItem) -> Result<JsonLiteral, EntityRefusal> {
    data_item(attribute, &item)?;
    match item {
        ExpandedItem::Json(value) => Ok(value),
        ExpandedItem::Value(_) | ExpandedItem::Node(_) | ExpandedItem::List(_) | ExpandedItem::Geometry(_) => Err(malformed(attribute, MemberShape::Json)),
    }
}

/// One language-map entry: a string tagged with its language, untagged for
/// `@none` (clause 4.5.18). A merge deletes one language with the NGSI-LD Null
/// (clause 5.5.12).
fn language_entry(attribute: &AttributeName, item: ExpandedItem) -> Result<LanguageEntry<Fragment>, EntityRefusal> {
    let deletion = is_null(&item);
    let ExpandedItem::Value(ValueObject {
        scalar: Scalar::String(text),
        datatype: None,
        language,
        ..
    }) = item
    else {
        return Err(malformed(attribute, MemberShape::LanguageMap));
    };
    let language = language
        .map(LangTagBuf::new)
        .transpose()
        .map_err(|_| malformed(attribute, MemberShape::LanguageMap))?;
    let text = if deletion {
        FragmentText::Remove
    } else {
        FragmentText::Text(CompactString::from(text))
    };
    Ok(LanguageEntry::new(language, text))
}

/// One `objectList` item: the bare target, or the target wrapped in `object`
/// (clause 4.5.22).
fn list_object(attribute: &AttributeName, item: &ExpandedItem) -> Result<ListObject, EntityRefusal> {
    if is_list_object_null(item) {
        return Err(null_beside_data(attribute));
    }
    let target = |text: &str| text.parse::<Uri>().map_err(|_| malformed(attribute, MemberShape::ObjectList));
    match item {
        ExpandedItem::Node(node) if node.is_reference() => node
            .id
            .as_deref()
            .map_or_else(|| Err(malformed(attribute, MemberShape::ObjectList)), target)
            .map(ListObject::Bare),
        ExpandedItem::Node(ExpandedNode { id: None, types, members }) if types.is_empty() => match members.as_slice() {
            [(name, objects)] if name == properties::OBJECT.as_str() => match objects.as_slice() {
                [ExpandedItem::Node(reference)] => reference_id(reference)
                    .map_or_else(|| Err(malformed(attribute, MemberShape::ObjectList)), target)
                    .map(ListObject::Wrapped),
                [] | [_, ..] => Err(malformed(attribute, MemberShape::ObjectList)),
            },
            [] | [_, ..] => Err(malformed(attribute, MemberShape::ObjectList)),
        },
        // The core context does not type `objectList` `@id`, so a concise
        // target written as a string expands to a value object.
        ExpandedItem::Value(value) => value
            .as_str()
            .map_or_else(|| Err(malformed(attribute, MemberShape::ObjectList)), target)
            .map(ListObject::Literal),
        ExpandedItem::Node(_) | ExpandedItem::Json(_) | ExpandedItem::List(_) | ExpandedItem::Geometry(_) => Err(malformed(attribute, MemberShape::ObjectList)),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{assembly::written_value::value_slot, assembly_error::AssemblyError};
    use andromeda_ngsi_ld_entity::{
        document::{
            value_carrier::ValueCarrier,
            written_value::{ValueSlot, WrittenValue},
        },
        member_shape::MemberShape,
        refusal::EntityRefusal,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};

    fn tags() -> AttributeName {
        "https://example.org/tags".parse().expect("an attribute name")
    }

    /// Clause 3.1: a present `value` member holding no item is the empty array,
    /// whether or not the instance declares its family (clause 5.6.4).
    #[test]
    fn a_value_member_holding_no_item_is_the_empty_array() {
        for family in [Some(AttributeType::Property), None] {
            let slot = value_slot(&tags(), family, ValueCarrier::Value, Vec::new()).expect("the empty array is a value");
            pretty_assertions::assert_eq!(slot, ValueSlot::Set(WrittenValue::Value(Vec::new())), "{family:?}");
        }
    }

    /// Every other member names at least one target, term, string or geometry.
    #[test]
    fn any_other_member_holding_no_item_is_malformed() {
        let cases = [
            (Some(AttributeType::GeoProperty), ValueCarrier::Value, MemberShape::Geometry),
            (None, ValueCarrier::Object, MemberShape::Object),
            (None, ValueCarrier::Vocab, MemberShape::Vocab),
            (None, ValueCarrier::LanguageMap, MemberShape::LanguageMap),
            (None, ValueCarrier::Json, MemberShape::Json),
            (None, ValueCarrier::ValueList, MemberShape::ValueList),
            (None, ValueCarrier::ObjectList, MemberShape::ObjectList),
        ];
        for (family, carrier, member) in cases {
            let refusal = value_slot(&tags(), family, carrier, Vec::new()).expect_err("an empty member names nothing");
            assert!(
                matches!(&refusal, AssemblyError::Refused(EntityRefusal::Malformed { attribute: Some(attribute), member: refused }) if *attribute == tags() && *refused == member),
                "{carrier:?}: {refusal:?}"
            );
        }
    }
}
