//! Reading the one-value members of an attribute instance: its times, its
//! identifiers, its unit and its type references.
//!
//! Each is written once (clause 5.2.5 and its siblings) and expands to one
//! value object, or to one node reference under the `@id`- and `@vocab`-typed
//! terms of the core context. A partial update or a merge may delete one with
//! the NGSI-LD Null (clauses 5.5.8, 5.5.12); a `datasetId` or an `instanceId`
//! never.

use crate::{
    assembly::null_sentinel::{is_null, is_sole_null},
    expanded_tree::{ExpandedItem, ExpandedNode},
};
use andromeda_ngsi_ld_entity::{document::slot::Slot, member_shape::MemberShape, refusal::EntityRefusal};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    identifier::Identifier,
    reference::NgsiLdReference,
    time::{date_time::NgsiLdDateTime, system_instant::SystemInstant},
    unit_code_resolution::{UnitCodeRefusal, resolve_unit_code},
};
use andromeda_ngsi_ld_vocabulary::keyword;
use cefact_units::UnitCode;

/// The refusal of a member not shaped as its expanded form shapes it.
#[must_use]
pub(crate) fn malformed(attribute: &AttributeName, member: MemberShape) -> EntityRefusal {
    EntityRefusal::Malformed {
        attribute: Some(attribute.clone()), // clone: the refusal outlives the borrowed attribute, so it owns a copy
        member,
    }
}

/// The refusal of the NGSI-LD Null standing beside data in one member.
///
/// Clause 4.5.0 reserves the Null for deletion, alone or as "an array with one
/// single NGSI-LD Null"; beside other values it neither deletes the member nor
/// is data, since "null cannot be used as a value in NGSI-LD".
#[must_use]
pub(crate) fn null_beside_data(attribute: &AttributeName) -> EntityRefusal {
    EntityRefusal::NullBesideData {
        attribute: Some(attribute.clone()), // clone: the refusal outlives the borrowed attribute, so it owns a copy
    }
}

/// The text of the one value object a member holds.
fn single_text<'a>(items: &'a [ExpandedItem], attribute: &AttributeName, member: MemberShape) -> Result<&'a str, EntityRefusal> {
    match items {
        [ExpandedItem::Value(value)] => value.as_str().ok_or_else(|| malformed(attribute, member)),
        [] | [_, ..] => Err(malformed(attribute, member)),
    }
}

/// The identifier of the one node reference a member holds.
fn single_reference<'a>(items: &'a [ExpandedItem], attribute: &AttributeName, member: MemberShape) -> Result<&'a str, EntityRefusal> {
    match items {
        [ExpandedItem::Node(node)] => reference_id(node).ok_or_else(|| malformed(attribute, member)),
        [] | [_, ..] => Err(malformed(attribute, member)),
    }
}

/// The identifier of a node reference.
#[must_use]
pub(crate) fn reference_id(node: &ExpandedNode) -> Option<&str> {
    if node.is_reference() { node.id.as_deref() } else { None }
}

/// A date-time member a write may delete (`observedAt`, `expiresAt`).
///
/// # Errors
///
/// Returns [`EntityRefusal::DateTime`] for a text that is not RFC 3339.
pub(crate) fn date_time_slot(items: &[ExpandedItem], attribute: &AttributeName, member: MemberShape) -> Result<Slot<NgsiLdDateTime>, EntityRefusal> {
    if is_sole_null(items) {
        return Ok(Slot::Remove);
    }
    date_time(items, attribute, member).map(Slot::Set)
}

/// A date-time member (`deletedAt`).
///
/// # Errors
///
/// Returns [`EntityRefusal::DateTime`] for a text that is not RFC 3339.
pub(crate) fn date_time(items: &[ExpandedItem], attribute: &AttributeName, member: MemberShape) -> Result<NgsiLdDateTime, EntityRefusal> {
    single_text(items, attribute, member)?
        .parse::<NgsiLdDateTime>()
        .map_err(|_| EntityRefusal::DateTime {
            attribute: attribute.clone(), // clone: the refusal outlives the borrowed attribute, so it owns a copy
            member,
        })
}

/// A broker-stamped time (`createdAt`, `modifiedAt`).
///
/// # Errors
///
/// Returns [`EntityRefusal::DateTime`] for a text that is not RFC 3339.
pub(crate) fn system_instant(items: &[ExpandedItem], attribute: &AttributeName, member: MemberShape) -> Result<SystemInstant, EntityRefusal> {
    single_text(items, attribute, member)?
        .parse::<SystemInstant>()
        .map_err(|_| EntityRefusal::DateTime {
            attribute: attribute.clone(), // clone: the refusal outlives the borrowed attribute, so it owns a copy
            member,
        })
}

/// The `datasetId` (clause 4.5.5), `None` for the default instance.
///
/// "If no datasetId is provided, or "datasetId": "@none" is supplied, it is
/// considered as the default Attribute instance", so the `@none` keyword is the
/// absent member.
///
/// # Errors
///
/// Returns [`EntityRefusal::DatasetIdSentinel`] for the NGSI-LD Null and
/// [`EntityRefusal::DatasetId`] for a text that is neither `@none` nor an
/// identifier.
pub(crate) fn dataset_id(items: &[ExpandedItem], attribute: &AttributeName) -> Result<Option<Identifier>, EntityRefusal> {
    if items.iter().any(is_null) {
        return Err(EntityRefusal::DatasetIdSentinel {
            attribute: attribute.clone(), // clone: the refusal outlives the borrowed attribute, so it owns a copy
        });
    }
    let text = single_reference(items, attribute, MemberShape::DatasetId)?;
    if text == keyword::NONE {
        return Ok(None);
    }
    text.parse::<Identifier>().map(Some).map_err(|source| EntityRefusal::DatasetId {
        attribute: attribute.clone(), // clone: the refusal outlives the borrowed attribute, so it owns a copy
        source,
    })
}

/// The `instanceId` (clause 4.5.7).
///
/// # Errors
///
/// Returns [`EntityRefusal::ReservedNullValue`] for the NGSI-LD Null and
/// [`EntityRefusal::InstanceId`] for a text that is not an identifier.
pub(crate) fn instance_id(items: &[ExpandedItem], attribute: &AttributeName) -> Result<Identifier, EntityRefusal> {
    if items.iter().any(is_null) {
        return Err(EntityRefusal::ReservedNullValue {
            attribute: attribute.clone(), // clone: the refusal outlives the borrowed attribute, so it owns a copy
        });
    }
    single_reference(items, attribute, MemberShape::InstanceId)?
        .parse::<Identifier>()
        .map_err(|source| EntityRefusal::InstanceId {
            attribute: attribute.clone(), // clone: the refusal outlives the borrowed attribute, so it owns a copy
            source,
        })
}

/// The `unitCode` (clause 4.5.2.2, Table 5.2.5-1).
///
/// # Errors
///
/// Returns [`EntityRefusal::UnitCodeUnusable`] for a member that is not one
/// UN/CEFACT common code.
pub(crate) fn unit_code_slot(items: &[ExpandedItem], attribute: &AttributeName) -> Result<Slot<UnitCode>, EntityRefusal> {
    if is_sole_null(items) {
        return Ok(Slot::Remove);
    }
    let unusable = |source| EntityRefusal::UnitCodeUnusable {
        attribute: attribute.clone(), // clone: the refusal outlives the borrowed attribute, so it owns a copy
        source,
    };
    let code = single_text(items, attribute, MemberShape::UnitCode).map_err(|_| unusable(UnitCodeRefusal::Malformed))?;
    resolve_unit_code(code).map(Slot::Set).map_err(unusable)
}

/// A `valueType` (clause 4.5.2.2).
///
/// # Errors
///
/// Returns [`EntityRefusal::Reference`] for a reference that is not a name.
pub(crate) fn reference_slot(items: &[ExpandedItem], attribute: &AttributeName, member: MemberShape) -> Result<Slot<NgsiLdReference>, EntityRefusal> {
    if is_sole_null(items) {
        return Ok(Slot::Remove);
    }
    reference(single_reference(items, attribute, member)?, attribute, member).map(Slot::Set)
}

/// An `objectType`: one reference or several (clause 4.5.3.2).
///
/// # Errors
///
/// Returns [`EntityRefusal::Reference`] for a reference that is not a name.
pub(crate) fn references_slot(items: &[ExpandedItem], attribute: &AttributeName, member: MemberShape) -> Result<Slot<Vec<NgsiLdReference>>, EntityRefusal> {
    if is_sole_null(items) {
        return Ok(Slot::Remove);
    }
    items
        .iter()
        .map(|item| match item {
            ExpandedItem::Node(node) => reference(reference_id(node).ok_or_else(|| malformed(attribute, member))?, attribute, member),
            ExpandedItem::Value(_) | ExpandedItem::Json(_) | ExpandedItem::List(_) | ExpandedItem::Geometry(_) => Err(malformed(attribute, member)),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Slot::Set)
}

/// One type reference.
fn reference(text: &str, attribute: &AttributeName, member: MemberShape) -> Result<NgsiLdReference, EntityRefusal> {
    text.parse::<NgsiLdReference>().map_err(|source| EntityRefusal::Reference {
        attribute: attribute.clone(), // clone: the refusal outlives the borrowed attribute, so it owns a copy
        member,
        source,
    })
}
