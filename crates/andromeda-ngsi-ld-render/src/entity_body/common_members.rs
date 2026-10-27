//! The members every attribute family shares, and the sub-attributes an
//! instance carries, written in a reified representation.

use crate::{
    entity_body::{
        member_walk::MemberWalk,
        notified_member::MemberInstances,
        reified_member::{Reification, ReifiedMember},
    },
    version::{deletion::DeletionEncoding, fallback::InstanceMember},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time::rendering::instant_text};
use andromeda_ngsi_ld_model::attribute::{common_fields::CommonAttributeFields, instance::AttributeValue};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Serialize, ser::SerializeMap};

/// Whether an instance carries nothing beside its family's own members,
/// counting only the members `keeps` says are written.
pub(crate) fn carries_nothing(common: &CommonAttributeFields, keeps: impl Fn(InstanceMember) -> bool) -> bool {
    common.created_at.is_none()
        && (common.dataset_id.is_none() || !keeps(InstanceMember::DatasetId))
        && common.deleted_at.is_none()
        && (common.expires_at.is_none() || !keeps(InstanceMember::ExpiresAt))
        && common.instance_id.is_none()
        && common.modified_at.is_none()
        && (common.observed_at.is_none() || !keeps(InstanceMember::ObservedAt))
        && common.sub_attributes.is_empty()
}

/// Write the members every attribute family shares that `keeps` says are
/// written, `deletedAt` the one a notification reports when it reports one.
pub(crate) fn serialize_common_members<M: SerializeMap>(
    object: &mut M,
    common: &CommonAttributeFields,
    reported_deleted_at: Option<DateTime<Utc>>,
    keeps: impl Fn(InstanceMember) -> bool,
) -> Result<(), M::Error> {
    serialize_present(object, compact_properties::CREATED_AT, common.created_at.map(instant_text).as_ref())?;
    serialize_present(
        object,
        compact_properties::DATASET_ID,
        common.dataset_id.as_ref().filter(|_| keeps(InstanceMember::DatasetId)),
    )?;
    match reported_deleted_at {
        Some(deleted_at) => object.serialize_entry(compact_properties::DELETED_AT, &instant_text(deleted_at))?,
        None => serialize_present(object, compact_properties::DELETED_AT, common.deleted_at.map(instant_text).as_ref())?,
    }
    serialize_present(
        object,
        compact_properties::EXPIRES_AT,
        common.expires_at.filter(|_| keeps(InstanceMember::ExpiresAt)).map(instant_text).as_ref(),
    )?;
    serialize_present(object, compact_properties::INSTANCE_ID, common.instance_id.as_ref())?;
    serialize_present(object, compact_properties::MODIFIED_AT, common.modified_at.map(instant_text).as_ref())?;
    serialize_present(
        object,
        compact_properties::OBSERVED_AT,
        common.observed_at.filter(|_| keeps(InstanceMember::ObservedAt)).map(instant_text).as_ref(),
    )
}

/// Write an instance's sub-attributes beside its own members.
pub(crate) fn serialize_sub_attributes<M: SerializeMap>(
    object: &mut M,
    sub_attributes: &IndexMap<AttributeName, AttributeValue>,
    reification: Reification,
    walk: MemberWalk<'_>,
) -> Result<(), M::Error> {
    sub_attributes.iter().try_for_each(|(name, value)| {
        serialize_attribute_member(
            object,
            name.as_str(),
            value,
            walk,
            &ReifiedMember::new(MemberInstances::Held(value), reification, walk),
        )
    })
}

/// Write the attribute member `name`, which `written` writes as `member`
/// holds it; the NGSI-LD Null deleting the member is written only as the
/// walk's target reads such a deletion, and left out when it reads none.
pub(crate) fn serialize_attribute_member<M: SerializeMap, W: Serialize>(
    object: &mut M,
    name: &str,
    member: &AttributeValue,
    walk: MemberWalk<'_>,
    written: &W,
) -> Result<(), M::Error> {
    match (member, walk.attribute_deletion()) {
        (AttributeValue::Single(_) | AttributeValue::Multiple(_), _) | (AttributeValue::NgsiLdNull, Some(DeletionEncoding::NgsiLdNull)) => {
            object.serialize_entry(name, written)
        }
        (AttributeValue::NgsiLdNull, Some(DeletionEncoding::JsonNull)) => object.serialize_entry(name, &()),
        (AttributeValue::NgsiLdNull, None) => Ok(()),
    }
}

/// Write a member when the instance carries it.
pub(crate) fn serialize_present<M: SerializeMap, T: Serialize>(object: &mut M, name: &str, value: Option<T>) -> Result<(), M::Error> {
    value.map_or(Ok(()), |value| object.serialize_entry(name, &value))
}
