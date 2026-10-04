//! Reading a written value from the elements of its carrying member.
//!
//! Each family's value has one expanded shape (clause 4.5): values for
//! `value`, `@id` references for `object` and `vocab`, language-tagged value
//! objects for `languageMap`, JSON literals for `json`, and one `@list` object
//! for `valueList` and `objectList`. The deletion encoding of each family
//! (clause 4.5.0) is recognised here and becomes [`ValueSlot::Remove`].

use crate::{
    codec::{
        element::Decoded,
        geometry_decode::{GeoDecoded, geo_value_of},
        member_decode::{malformed, reference_text},
    },
    document::{
        value_carrier::ValueCarrier,
        written_value::{ValueSlot, WrittenValue},
    },
    expanded_value::ExpandedValue,
    json_literal::JsonLiteral,
    language_entries::{LanguageEntries, LanguageEntry},
    list_object::ListObject,
    literal::{Literal, LiteralValue},
    member_shape::MemberShape,
    node_id::NodeId,
    non_empty::NonEmpty,
    refusal::EntityRefusal,
    value_mode::{Fragment, FragmentMember, FragmentText},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, reference::NgsiLdReference, uri::Uri};
use andromeda_ngsi_ld_vocabulary::expanded::properties;

/// What was read from a carrying member.
#[derive(Debug)]
pub(crate) enum CarrierContent {
    /// The generic elements of the member.
    Elements(Vec<Decoded>),
    /// The `value` of an instance known to be a `GeoProperty`, read as one.
    Geometry(GeoDecoded),
}

/// The written value `content` makes under `carrier`, for an instance
/// declaring `family`.
pub(crate) fn written_value(
    carrier: ValueCarrier,
    content: CarrierContent,
    family: Option<AttributeType>,
    attribute: &AttributeName,
) -> Result<ValueSlot, EntityRefusal> {
    let elements = match content {
        CarrierContent::Geometry(GeoDecoded::Null) => return Ok(ValueSlot::Remove(carrier)),
        CarrierContent::Geometry(GeoDecoded::Value(geometry)) => return Ok(ValueSlot::Set(WrittenValue::Geometry(geometry))),
        CarrierContent::Elements(elements) => elements,
    };
    if matches!(elements.as_slice(), [Decoded::Null]) {
        return Ok(ValueSlot::Remove(carrier));
    }
    let shape = carrier_shape(carrier);
    let value = match carrier {
        ValueCarrier::Value if family == Some(AttributeType::GeoProperty) => {
            let values = values(elements, attribute)?;
            match geo_value_of(attribute, values.as_slice())? {
                GeoDecoded::Value(geometry) => WrittenValue::Geometry(geometry),
                GeoDecoded::Null => return Ok(ValueSlot::Remove(carrier)),
            }
        }
        ValueCarrier::Value => WrittenValue::Value(values(elements, attribute)?),
        ValueCarrier::Object => WrittenValue::Object(non_empty(uris(elements, attribute, shape)?, attribute, shape)?),
        ValueCarrier::LanguageMap => WrittenValue::LanguageMap(language_entries(elements, attribute)?),
        ValueCarrier::Vocab => WrittenValue::Vocab(non_empty(names(elements, attribute, shape)?, attribute, shape)?),
        ValueCarrier::Json => WrittenValue::Json(non_empty(json_literals(elements, attribute)?, attribute, shape)?),
        ValueCarrier::ValueList => match list_items(elements, attribute, shape)? {
            ListContent::Removed => return Ok(ValueSlot::Remove(carrier)),
            ListContent::Items(items) => WrittenValue::ValueList(items),
        },
        ValueCarrier::ObjectList => match list_items(elements, attribute, shape)? {
            ListContent::Removed => return Ok(ValueSlot::Remove(carrier)),
            ListContent::Items(items) => match list_objects(&items, attribute)? {
                Some(objects) => WrittenValue::ObjectList(objects),
                None => return Ok(ValueSlot::Remove(carrier)),
            },
        },
    };
    Ok(ValueSlot::Set(value))
}

/// The member shape a carrier's refusal names.
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

/// `elements` as one or more, refusing none.
fn non_empty<T>(elements: Vec<T>, attribute: &AttributeName, shape: MemberShape) -> Result<NonEmpty<T>, EntityRefusal> {
    NonEmpty::try_from_vec(elements).map_err(|_| malformed(Some(attribute), shape))
}

/// The values of the member, refusing a Null among them.
fn values(elements: Vec<Decoded>, attribute: &AttributeName) -> Result<Vec<ExpandedValue<Fragment>>, EntityRefusal> {
    elements
        .into_iter()
        .map(|element| match element {
            Decoded::Value(value) => Ok(value),
            Decoded::Null | Decoded::TaggedNull(_) | Decoded::NullList => Err(EntityRefusal::NullBesideData {
                attribute: Some(attribute.clone()),
            }),
        })
        .collect()
}

/// The `@id` references of the member, as URIs.
fn uris(elements: Vec<Decoded>, attribute: &AttributeName, shape: MemberShape) -> Result<Vec<Uri>, EntityRefusal> {
    values(elements, attribute)?
        .into_iter()
        .map(|value| reference_uri(&value).ok_or_else(|| malformed(Some(attribute), shape)))
        .collect()
}

/// The URI a node reference names.
fn reference_uri(value: &ExpandedValue<Fragment>) -> Option<Uri> {
    let ExpandedValue::Node(node) = value else {
        return None;
    };
    match node.id() {
        Some(NodeId::Uri(uri)) if node.is_reference() => Some(uri.clone()),
        Some(NodeId::Uri(_) | NodeId::Blank(_) | NodeId::Verbatim(_)) | None => None,
    }
}

/// The `@id` references of the member, as names.
fn names(elements: Vec<Decoded>, attribute: &AttributeName, shape: MemberShape) -> Result<Vec<NgsiLdReference>, EntityRefusal> {
    values(elements, attribute)?
        .into_iter()
        .map(|value| match &value {
            ExpandedValue::Node(node) => reference_text(node)
                .ok_or_else(|| malformed(Some(attribute), shape))?
                .parse::<NgsiLdReference>()
                .map_err(|source| EntityRefusal::Reference {
                    attribute: attribute.clone(),
                    member: shape,
                    source,
                }),
            ExpandedValue::Literal(_) | ExpandedValue::Json(_) | ExpandedValue::List(_) => Err(malformed(Some(attribute), shape)),
        })
        .collect()
}

/// The entries of a language map (clause 4.5.18); a tagged Null deletes one
/// language, an untagged one the `@none` entry.
fn language_entries(elements: Vec<Decoded>, attribute: &AttributeName) -> Result<LanguageEntries<Fragment>, EntityRefusal> {
    elements
        .into_iter()
        .map(|element| match element {
            Decoded::Null => Ok(LanguageEntry::new(None, FragmentText::Remove)),
            Decoded::TaggedNull(language) => Ok(LanguageEntry::new(Some(language), FragmentText::Remove)),
            Decoded::Value(ExpandedValue::Literal(literal)) if literal.datatype().is_none() && literal.direction().is_none() => match literal.value() {
                LiteralValue::String(text) => Ok(LanguageEntry::new(literal.language().cloned(), FragmentText::Text(text.clone()))),
                LiteralValue::Number(_) | LiteralValue::Boolean(_) => Err(malformed(Some(attribute), MemberShape::LanguageMap)),
            },
            Decoded::Value(ExpandedValue::Literal(_) | ExpandedValue::Node(_) | ExpandedValue::Json(_) | ExpandedValue::List(_)) | Decoded::NullList => {
                Err(malformed(Some(attribute), MemberShape::LanguageMap))
            }
        })
        .collect::<Result<Vec<_>, _>>()
        .map(LanguageEntries::new)
}

/// The JSON literals of a `json` member (clause 4.5.24).
fn json_literals(elements: Vec<Decoded>, attribute: &AttributeName) -> Result<Vec<JsonLiteral>, EntityRefusal> {
    values(elements, attribute)?
        .into_iter()
        .map(|value| match value {
            ExpandedValue::Json(json) => Ok(json),
            ExpandedValue::Literal(_) | ExpandedValue::Node(_) | ExpandedValue::List(_) => Err(malformed(Some(attribute), MemberShape::Json)),
        })
        .collect()
}

/// What one `@list` member holds.
enum ListContent {
    /// The list families' deletion encoding.
    Removed,
    /// The items, in order.
    Items(Vec<ExpandedValue<Fragment>>),
}

/// The one list object of a `valueList` or `objectList` member.
fn list_items(elements: Vec<Decoded>, attribute: &AttributeName, shape: MemberShape) -> Result<ListContent, EntityRefusal> {
    let mut elements = elements.into_iter();
    match (elements.next(), elements.next()) {
        (Some(Decoded::NullList), None) => Ok(ListContent::Removed),
        (Some(Decoded::Value(ExpandedValue::List(items))), None) => Ok(ListContent::Items(items)),
        (Some(Decoded::Value(ExpandedValue::Literal(_) | ExpandedValue::Node(_) | ExpandedValue::Json(_)) | Decoded::Null | Decoded::TaggedNull(_)), None)
        | (None, _)
        | (Some(_), Some(_)) => Err(malformed(Some(attribute), shape)),
    }
}

/// The items of an object list, each in its stored spelling; `None` for a
/// list holding only the wrapped Null, the deletion encoding of clause
/// 4.5.22.2.
fn list_objects(items: &[ExpandedValue<Fragment>], attribute: &AttributeName) -> Result<Option<Vec<ListObject>>, EntityRefusal> {
    if let [ExpandedValue::Node(node)] = items
        && node.id().is_none()
        && node.types().is_empty()
        && node.members().len() == 1
        && matches!(node.members().get(properties::OBJECT.as_str()), Some(FragmentMember::Remove))
    {
        return Ok(None);
    }
    items
        .iter()
        .map(|item| list_object(item).ok_or_else(|| malformed(Some(attribute), MemberShape::ObjectList)))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

/// The target of a plain string value object, the spelling of a concise
/// object-list item under a context that does not type `objectList` `@id`.
fn literal_uri(literal: &Literal) -> Option<ListObject> {
    if literal.datatype().is_some() || literal.language().is_some() || literal.direction().is_some() {
        return None;
    }
    match literal.value() {
        LiteralValue::String(text) => text.parse::<Uri>().ok().map(ListObject::Literal),
        LiteralValue::Number(_) | LiteralValue::Boolean(_) => None,
    }
}

/// One object-list item: a bare reference, a plain string, or one wrapped in `hasObject`.
fn list_object(item: &ExpandedValue<Fragment>) -> Option<ListObject> {
    if let Some(uri) = reference_uri(item) {
        return Some(ListObject::Bare(uri));
    }
    let node = match item {
        ExpandedValue::Node(node) => node,
        ExpandedValue::Literal(literal) => return literal_uri(literal),
        ExpandedValue::Json(_) | ExpandedValue::List(_) => return None,
    };
    if node.id().is_some() || !node.types().is_empty() || node.members().len() != 1 {
        return None;
    }
    match node.members().get(properties::OBJECT.as_str())? {
        FragmentMember::Values(values) => match values.as_slice() {
            [target] => reference_uri(target).map(ListObject::Wrapped),
            [] | [_, _, ..] => None,
        },
        FragmentMember::Remove => None,
    }
}
