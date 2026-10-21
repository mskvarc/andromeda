//! Reading the reserved members of an attribute instance and of an entity from
//! their expanded elements.
//!
//! Each member has one expanded shape under the core context: an `@id`
//! reference for `datasetId`, `instanceId`, `valueType` and `objectType`, a
//! `DateTime`-typed value object for the temporal members, a plain value object
//! for `unitCode` and each `scope` path. The NGSI-LD Null, where a member
//! admits it, is the deletion of that member (clauses 5.5.8, 5.5.12).

use crate::{
    codec::element::{Decoded, values_without_null},
    conversion::stored_value::into_stored_values,
    document::slot::Slot,
    expanded_value::{ExpandedValue, ValueNode},
    literal::{Literal, LiteralValue},
    member_shape::MemberShape,
    node_id::NodeId,
    refusal::EntityRefusal,
    scope_slot::{ScopeItem, scope_slot},
    scopes::Scopes,
    value_mode::Fragment,
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    identifier::Identifier,
    reference::NgsiLdReference,
    time::{system_instant::SystemInstant, written_date_time::DateTimeText},
    unit_code_resolution::{UnitCodeRefusal, resolve_unit_code},
};
use andromeda_ngsi_ld_vocabulary::expanded::classes;
use cefact_units::UnitCode;

/// The refusal of a member not shaped as its expanded form.
pub(crate) fn malformed(attribute: Option<&AttributeName>, member: MemberShape) -> EntityRefusal {
    EntityRefusal::Malformed {
        attribute: attribute.cloned(),
        member,
    }
}

/// The IRI text of a node reference: a node carrying nothing but its `@id`.
pub(crate) fn reference_text(node: &ValueNode<Fragment>) -> Option<&str> {
    if node.is_reference() { node.id().map(NodeId::as_str) } else { None }
}

/// The one element of a single-valued member.
fn single(elements: Vec<Decoded>, attribute: Option<&AttributeName>, member: MemberShape) -> Result<Decoded, EntityRefusal> {
    let mut elements = elements.into_iter();
    match (elements.next(), elements.next()) {
        (Some(element), None) => Ok(element),
        (None, _) | (Some(_), Some(_)) => Err(malformed(attribute, member)),
    }
}

/// The text of a plain string value object, or of one typed `DateTime`.
fn string_literal(literal: &Literal) -> Option<&str> {
    let datatype_admitted = literal.datatype().is_none_or(|datatype| datatype.as_str() == classes::DATE_TIME.as_str());
    match literal.value() {
        LiteralValue::String(text) if datatype_admitted && literal.language().is_none() && literal.direction().is_none() => Some(text),
        LiteralValue::String(_) | LiteralValue::Number(_) | LiteralValue::Boolean(_) => None,
    }
}

/// `datasetId`: an identifier, never the Null (clauses 5.5.8, 5.5.12).
pub(crate) fn dataset_id(elements: Vec<Decoded>, attribute: &AttributeName) -> Result<Identifier, EntityRefusal> {
    match single(elements, Some(attribute), MemberShape::DatasetId)? {
        Decoded::Null => Err(EntityRefusal::DatasetIdSentinel { attribute: attribute.clone() }),
        Decoded::Value(ExpandedValue::Node(node)) => {
            let text = reference_text(&node).ok_or_else(|| malformed(Some(attribute), MemberShape::DatasetId))?;
            text.parse::<Identifier>().map_err(|source| EntityRefusal::DatasetId {
                attribute: attribute.clone(),
                source,
            })
        }
        Decoded::Value(ExpandedValue::Literal(_) | ExpandedValue::Json(_) | ExpandedValue::List(_)) | Decoded::TaggedNull(_) | Decoded::NullList => {
            Err(malformed(Some(attribute), MemberShape::DatasetId))
        }
    }
}

/// `instanceId`: an identifier (clause 4.5.7).
pub(crate) fn instance_id(elements: Vec<Decoded>, attribute: &AttributeName) -> Result<Identifier, EntityRefusal> {
    match single(elements, Some(attribute), MemberShape::InstanceId)? {
        Decoded::Value(ExpandedValue::Node(node)) => {
            let text = reference_text(&node).ok_or_else(|| malformed(Some(attribute), MemberShape::InstanceId))?;
            text.parse::<Identifier>().map_err(|source| EntityRefusal::InstanceId {
                attribute: attribute.clone(),
                source,
            })
        }
        Decoded::Null => Err(EntityRefusal::ReservedNullValue { attribute: attribute.clone() }),
        Decoded::Value(ExpandedValue::Literal(_) | ExpandedValue::Json(_) | ExpandedValue::List(_)) | Decoded::TaggedNull(_) | Decoded::NullList => {
            Err(malformed(Some(attribute), MemberShape::InstanceId))
        }
    }
}

/// A client-written temporal member, or its deletion.
pub(crate) fn date_time(elements: Vec<Decoded>, attribute: Option<&AttributeName>, member: MemberShape) -> Result<Slot<DateTimeText>, EntityRefusal> {
    match single(elements, attribute, member)? {
        Decoded::Null => Ok(Slot::Remove),
        Decoded::Value(ExpandedValue::Literal(literal)) => {
            let text = string_literal(&literal).ok_or_else(|| malformed(attribute, member))?;
            text.parse::<DateTimeText>().map(Slot::Set).map_err(|_| match attribute {
                Some(attribute) => EntityRefusal::DateTime {
                    attribute: attribute.clone(),
                    member,
                },
                None => EntityRefusal::EntityExpiresAt,
            })
        }
        Decoded::Value(ExpandedValue::Node(_) | ExpandedValue::Json(_) | ExpandedValue::List(_)) | Decoded::TaggedNull(_) | Decoded::NullList => {
            Err(malformed(attribute, member))
        }
    }
}

/// A broker-stamped system time (clause 4.8).
pub(crate) fn system_instant(elements: Vec<Decoded>, attribute: &AttributeName, member: MemberShape) -> Result<SystemInstant, EntityRefusal> {
    match date_time(elements, Some(attribute), member)? {
        Slot::Set(text) => Ok(SystemInstant::new(text.instant())),
        Slot::Remove => Err(EntityRefusal::ReservedNullValue { attribute: attribute.clone() }),
    }
}

/// `unitCode`: a UN/CEFACT common code (Table 5.2.5-1), or its deletion.
pub(crate) fn unit_code(elements: Vec<Decoded>, attribute: &AttributeName) -> Result<Slot<UnitCode>, EntityRefusal> {
    let unusable = |source: UnitCodeRefusal| EntityRefusal::UnitCodeUnusable {
        attribute: attribute.clone(),
        source,
    };
    let element = single(elements, Some(attribute), MemberShape::UnitCode).map_err(|_| unusable(UnitCodeRefusal::Malformed))?;
    match element {
        Decoded::Null => Ok(Slot::Remove),
        Decoded::Value(ExpandedValue::Literal(literal)) if literal.datatype().is_none() && literal.language().is_none() && literal.direction().is_none() => {
            let LiteralValue::String(code) = literal.value() else {
                return Err(unusable(UnitCodeRefusal::Malformed));
            };
            resolve_unit_code(code).map(Slot::Set).map_err(unusable)
        }
        Decoded::Value(ExpandedValue::Literal(_) | ExpandedValue::Node(_) | ExpandedValue::Json(_) | ExpandedValue::List(_))
        | Decoded::TaggedNull(_)
        | Decoded::NullList => Err(unusable(UnitCodeRefusal::Malformed)),
    }
}

/// The names of `@id` references: `valueType`, `objectType`, `vocab`.
pub(crate) fn references(elements: Vec<Decoded>, attribute: &AttributeName, member: MemberShape) -> Result<Slot<Vec<NgsiLdReference>>, EntityRefusal> {
    if matches!(elements.as_slice(), [Decoded::Null]) {
        return Ok(Slot::Remove);
    }
    elements
        .into_iter()
        .map(|element| match element {
            Decoded::Value(ExpandedValue::Node(node)) => reference_text(&node)
                .ok_or_else(|| malformed(Some(attribute), member))?
                .parse::<NgsiLdReference>()
                .map_err(|source| EntityRefusal::Reference {
                    attribute: attribute.clone(),
                    member,
                    source,
                }),
            Decoded::Null | Decoded::TaggedNull(_) | Decoded::NullList => Err(EntityRefusal::NullBesideData {
                attribute: Some(attribute.clone()),
            }),
            Decoded::Value(ExpandedValue::Literal(_) | ExpandedValue::Json(_) | ExpandedValue::List(_)) => Err(malformed(Some(attribute), member)),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Slot::Set)
}

/// `valueType`: one name, or its deletion.
pub(crate) fn value_type(elements: Vec<Decoded>, attribute: &AttributeName) -> Result<Slot<NgsiLdReference>, EntityRefusal> {
    match references(elements, attribute, MemberShape::ValueType)? {
        Slot::Remove => Ok(Slot::Remove),
        Slot::Set(names) => {
            let mut names = names.into_iter();
            match (names.next(), names.next()) {
                (Some(name), None) => Ok(Slot::Set(name)),
                (None, _) | (Some(_), Some(_)) => Err(malformed(Some(attribute), MemberShape::ValueType)),
            }
        }
    }
}

/// The entity `scope` (clause 4.18): its paths, or its deletion.
pub(crate) fn scope(elements: &[Decoded]) -> Result<Slot<Scopes>, EntityRefusal> {
    scope_slot(elements.iter().map(|element| match element {
        Decoded::Null => ScopeItem::Null,
        Decoded::Value(ExpandedValue::Literal(literal)) => match literal.value() {
            LiteralValue::String(text) => ScopeItem::Text(text),
            LiteralValue::Number(_) | LiteralValue::Boolean(_) => ScopeItem::NotAString,
        },
        Decoded::Value(ExpandedValue::Node(_) | ExpandedValue::Json(_) | ExpandedValue::List(_)) | Decoded::TaggedNull(_) | Decoded::NullList => {
            ScopeItem::NotAString
        }
    }))
    .map_err(EntityRefusal::Scope)
}

/// The values of a data-integrity seal (clause 4.5.2.2), which carry no
/// deletion.
pub(crate) fn seal_values(elements: Vec<Decoded>, attribute: &AttributeName) -> Result<Vec<ExpandedValue>, EntityRefusal> {
    let values = values_without_null(elements, Some(attribute))?;
    into_stored_values(values).ok_or_else(|| EntityRefusal::NullInState {
        attribute: Some(attribute.clone()),
    })
}
