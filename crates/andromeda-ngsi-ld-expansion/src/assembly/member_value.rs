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
use andromeda_ngsi_ld_entity::{
    date_time_text::DateTimeText,
    document::slot::Slot,
    member_shape::MemberShape,
    refusal::EntityRefusal,
    system_instant::SystemInstant,
    unit_code::UnitCode,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, reference::NgsiLdReference};
use andromeda_unit_catalog::expanded_unit_code::{ExpandedUnitCodeError, UnitCodeRefusal};

/// The refusal of a member not shaped as its expanded form shapes it.
#[must_use]
pub fn malformed(attribute: &AttributeName, member: MemberShape) -> EntityRefusal {
    EntityRefusal::Malformed {
        attribute: Some(attribute.clone()), // clone: the refusal names the attribute the caller keeps
        member,
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
pub fn reference_id(node: &ExpandedNode) -> Option<&str> {
    if node.is_reference() { node.id.as_deref() } else { None }
}

/// A date-time member a write may delete (`observedAt`, `expiresAt`).
///
/// # Errors
///
/// Returns [`EntityRefusal::DateTime`] for a text that is not RFC 3339.
pub fn date_time_slot(items: &[ExpandedItem], attribute: &AttributeName, member: MemberShape) -> Result<Slot<DateTimeText>, EntityRefusal> {
    if is_sole_null(items) {
        return Ok(Slot::Remove);
    }
    date_time(items, attribute, member).map(Slot::Set)
}

/// A date-time member kept as written (`deletedAt`).
///
/// # Errors
///
/// Returns [`EntityRefusal::DateTime`] for a text that is not RFC 3339.
pub fn date_time(items: &[ExpandedItem], attribute: &AttributeName, member: MemberShape) -> Result<DateTimeText, EntityRefusal> {
    single_text(items, attribute, member)?
        .parse::<DateTimeText>()
        .map_err(|_| EntityRefusal::DateTime {
            attribute: attribute.clone(), // clone: the refusal names the attribute the caller keeps
            member,
        })
}

/// A broker-stamped time (`createdAt`, `modifiedAt`).
///
/// # Errors
///
/// Returns [`EntityRefusal::DateTime`] for a text that is not RFC 3339.
pub fn system_instant(items: &[ExpandedItem], attribute: &AttributeName, member: MemberShape) -> Result<SystemInstant, EntityRefusal> {
    single_text(items, attribute, member)?
        .parse::<SystemInstant>()
        .map_err(|_| EntityRefusal::DateTime {
            attribute: attribute.clone(), // clone: the refusal names the attribute the caller keeps
            member,
        })
}

/// The `datasetId` (clause 4.5.5).
///
/// # Errors
///
/// Returns [`EntityRefusal::DatasetIdSentinel`] for the NGSI-LD Null and
/// [`EntityRefusal::DatasetId`] for a text that is not an identifier.
pub fn dataset_id(items: &[ExpandedItem], attribute: &AttributeName) -> Result<Identifier, EntityRefusal> {
    if items.iter().any(is_null) {
        return Err(EntityRefusal::DatasetIdSentinel {
            attribute: attribute.clone(), // clone: the refusal names the attribute the caller keeps
        });
    }
    single_reference(items, attribute, MemberShape::DatasetId)?
        .parse::<Identifier>()
        .map_err(|source| EntityRefusal::DatasetId {
            attribute: attribute.clone(), // clone: the refusal names the attribute the caller keeps
            source,
        })
}

/// The `instanceId` (clause 4.5.7).
///
/// # Errors
///
/// Returns [`EntityRefusal::ReservedNullValue`] for the NGSI-LD Null and
/// [`EntityRefusal::InstanceId`] for a text that is not an identifier.
pub fn instance_id(items: &[ExpandedItem], attribute: &AttributeName) -> Result<Identifier, EntityRefusal> {
    if items.iter().any(is_null) {
        return Err(EntityRefusal::ReservedNullValue {
            attribute: attribute.clone(), // clone: the refusal names the attribute the caller keeps
        });
    }
    single_reference(items, attribute, MemberShape::InstanceId)?
        .parse::<Identifier>()
        .map_err(|source| EntityRefusal::InstanceId {
            attribute: attribute.clone(), // clone: the refusal names the attribute the caller keeps
            source,
        })
}

/// The `unitCode` (clause 4.5.2.2, Table 5.2.5-1).
///
/// # Errors
///
/// Returns [`EntityRefusal::UnitCodeUnusable`] for a member that is not one
/// common code the catalog registers, and [`EntityRefusal::UnitCatalog`] when
/// the compiled-in catalog does not parse.
pub fn unit_code_slot(items: &[ExpandedItem], attribute: &AttributeName) -> Result<Slot<UnitCode>, EntityRefusal> {
    if is_sole_null(items) {
        return Ok(Slot::Remove);
    }
    let unusable = |source| EntityRefusal::UnitCodeUnusable {
        attribute: attribute.clone(), // clone: the refusal names the attribute the caller keeps
        source,
    };
    let code = single_text(items, attribute, MemberShape::UnitCode).map_err(|_| unusable(UnitCodeRefusal::Malformed))?;
    UnitCode::resolve(code).map(Slot::Set).map_err(|error| match error {
        ExpandedUnitCodeError::Refused(refusal) => unusable(refusal),
        ExpandedUnitCodeError::Catalog(catalog) => EntityRefusal::UnitCatalog(catalog),
    })
}

/// A `valueType` (clause 4.5.2.2).
///
/// # Errors
///
/// Returns [`EntityRefusal::Reference`] for a reference that is not a name.
pub fn reference_slot(items: &[ExpandedItem], attribute: &AttributeName, member: MemberShape) -> Result<Slot<NgsiLdReference>, EntityRefusal> {
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
pub fn references_slot(items: &[ExpandedItem], attribute: &AttributeName, member: MemberShape) -> Result<Slot<Vec<NgsiLdReference>>, EntityRefusal> {
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
        attribute: attribute.clone(), // clone: the refusal names the attribute the caller keeps
        member,
        source,
    })
}
