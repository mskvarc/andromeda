//! What each member of a compact attribute instance is to the instance's
//! family (ETSI GS CIM 009 clause 5.2 and Tables 5.2.5-1 to 5.2.38-1).
//!
//! An instance names its family in `type`. Each family has a member holding
//! its value and one holding the previous value `showChanges` reports, and
//! some have members of their own: `unitCode` on a Property, `valueType` on the
//! property families, `objectType` and the inline `entity` / `entityList` of a
//! clause 4.5.23 join on the relationship families. The system members of
//! clauses 4.5.5, 4.5.7, 4.8 and 4.22 belong to every family, and every other
//! member is a sub-attribute (clause 4.5.6). What a member is depends on the
//! family alone: a `unitCode` beside a `ListProperty` value is a sub-attribute
//! of that name, exactly as the family's own members are in a decoded
//! document.

use crate::{
    attribute::{common_fields::CommonAttributeFields, geo_property::TYPE_MEMBER, instance::AttributeValue},
    compact_member::decoded_optional,
    compact_node::CompactNode,
    node_refusal::{NodeReadError, NodeRefusal, RefusedMember},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use indexmap::IndexMap;
use serde::de::DeserializeOwned;
use std::str::FromStr;

/// What one member of a compact instance is to its family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InstanceMember {
    /// `type`, which names the family.
    Family,
    /// The member holding the family's value.
    Value,
    /// The member holding the family's previous value.
    Previous,
    /// `unitCode`, a Property's own.
    UnitCode,
    /// `valueType`, the property families' own.
    ValueType,
    /// `objectType`, the relationship families' own.
    ObjectType,
    /// `entity` or `entityList`, the linked entities of a join.
    Inline,
    /// `createdAt`.
    CreatedAt,
    /// `datasetId`.
    DatasetId,
    /// `deletedAt`.
    DeletedAt,
    /// `expiresAt`.
    ExpiresAt,
    /// `instanceId`.
    InstanceId,
    /// `modifiedAt`.
    ModifiedAt,
    /// `observedAt`.
    ObservedAt,
    /// Any other member: a sub-attribute.
    SubAttribute,
}

impl InstanceMember {
    /// What the member named `name` is to `family`.
    fn of(family: AttributeType, name: &str) -> InstanceMember {
        match (name, family) {
            (TYPE_MEMBER, _) => InstanceMember::Family,
            (own, family) if own == family.compact_value_key() => InstanceMember::Value,
            (own, family) if own == family.previous_key() => InstanceMember::Previous,
            (compact_properties::UNIT_CODE, AttributeType::Property) => InstanceMember::UnitCode,
            (
                compact_properties::VALUE_TYPE,
                AttributeType::Property
                | AttributeType::GeoProperty
                | AttributeType::VocabProperty
                | AttributeType::LanguageProperty
                | AttributeType::JsonProperty
                | AttributeType::ListProperty,
            ) => InstanceMember::ValueType,
            (compact_properties::OBJECT_TYPE, AttributeType::Relationship | AttributeType::ListRelationship) => InstanceMember::ObjectType,
            (compact_properties::ENTITY, AttributeType::Relationship) | (compact_properties::ENTITY_LIST, AttributeType::ListRelationship) => {
                InstanceMember::Inline
            }
            (compact_properties::CREATED_AT, _) => InstanceMember::CreatedAt,
            (compact_properties::DATASET_ID, _) => InstanceMember::DatasetId,
            (compact_properties::DELETED_AT, _) => InstanceMember::DeletedAt,
            (compact_properties::EXPIRES_AT, _) => InstanceMember::ExpiresAt,
            (compact_properties::INSTANCE_ID, _) => InstanceMember::InstanceId,
            (compact_properties::MODIFIED_AT, _) => InstanceMember::ModifiedAt,
            (compact_properties::OBSERVED_AT, _) => InstanceMember::ObservedAt,
            (_other, _) => InstanceMember::SubAttribute,
        }
    }
}

/// The members of one compact instance, by what its family makes of them.
#[derive(Debug)]
pub(crate) struct InstanceMembers<'n, N> {
    /// The family's value member.
    pub(crate) value: Option<&'n N>,
    /// The family's previous-value member.
    pub(crate) previous: Option<&'n N>,
    /// `unitCode`.
    pub(crate) unit_code: Option<&'n N>,
    /// `valueType`.
    pub(crate) value_type: Option<&'n N>,
    /// `objectType`.
    pub(crate) object_type: Option<&'n N>,
    /// `entity` or `entityList`.
    pub(crate) inline: Option<&'n N>,
    /// The system members, by kind.
    system: SystemMembers<'n, N>,
    /// The sub-attributes, by name, in the order of their names.
    sub_attributes: Vec<(&'n str, &'n N)>,
}

/// The system members every family carries.
#[derive(Debug)]
struct SystemMembers<'n, N> {
    /// `createdAt`.
    created_at: Option<&'n N>,
    /// `datasetId`.
    dataset_id: Option<&'n N>,
    /// `deletedAt`.
    deleted_at: Option<&'n N>,
    /// `expiresAt`.
    expires_at: Option<&'n N>,
    /// `instanceId`.
    instance_id: Option<&'n N>,
    /// `modifiedAt`.
    modified_at: Option<&'n N>,
    /// `observedAt`.
    observed_at: Option<&'n N>,
}

impl<'n, N: CompactNode> InstanceMembers<'n, N> {
    /// Sort the members of an instance of `family`.
    pub(crate) fn sorted(family: AttributeType, members: impl Iterator<Item = (&'n str, &'n N)>) -> InstanceMembers<'n, N> {
        let mut sorted = InstanceMembers {
            value: None,
            previous: None,
            unit_code: None,
            value_type: None,
            object_type: None,
            inline: None,
            system: SystemMembers {
                created_at: None,
                dataset_id: None,
                deleted_at: None,
                expires_at: None,
                instance_id: None,
                modified_at: None,
                observed_at: None,
            },
            sub_attributes: Vec::new(),
        };
        for (name, member) in members {
            let slot = match InstanceMember::of(family, name) {
                InstanceMember::Family => continue,
                InstanceMember::Value => &mut sorted.value,
                InstanceMember::Previous => &mut sorted.previous,
                InstanceMember::UnitCode => &mut sorted.unit_code,
                InstanceMember::ValueType => &mut sorted.value_type,
                InstanceMember::ObjectType => &mut sorted.object_type,
                InstanceMember::Inline => &mut sorted.inline,
                InstanceMember::CreatedAt => &mut sorted.system.created_at,
                InstanceMember::DatasetId => &mut sorted.system.dataset_id,
                InstanceMember::DeletedAt => &mut sorted.system.deleted_at,
                InstanceMember::ExpiresAt => &mut sorted.system.expires_at,
                InstanceMember::InstanceId => &mut sorted.system.instance_id,
                InstanceMember::ModifiedAt => &mut sorted.system.modified_at,
                InstanceMember::ObservedAt => &mut sorted.system.observed_at,
                InstanceMember::SubAttribute => {
                    sorted.sub_attributes.push((name, member));
                    continue;
                }
            };
            *slot = Some(member);
        }
        sorted
    }

    /// The members every family shares: the system members and the
    /// sub-attributes.
    ///
    /// # Errors
    ///
    /// Returns [`NodeRefusal::Member`] for a system member that does not
    /// decode, [`NodeRefusal::AttributeName`] for a sub-attribute named by no
    /// attribute name, and the node's [`CompactNode::Unreadable`].
    pub(crate) fn common(&self) -> Result<CommonAttributeFields, NodeReadError<N::Unreadable>> {
        let system = &self.system;
        let mut sub_attributes = IndexMap::with_capacity(self.sub_attributes.len());
        for (name, member) in &self.sub_attributes {
            let name = AttributeName::from_str(name).map_err(NodeRefusal::AttributeName)?;
            sub_attributes.insert(name, AttributeValue::from_node(*member).map_err(NodeReadError::Unreadable)?);
        }
        Ok(CommonAttributeFields {
            created_at: optional_member(system.created_at, RefusedMember::CreatedAt)?,
            dataset_id: optional_member(system.dataset_id, RefusedMember::DatasetId)?,
            deleted_at: optional_member(system.deleted_at, RefusedMember::DeletedAt)?,
            expires_at: optional_member(system.expires_at, RefusedMember::ExpiresAt)?,
            instance_id: optional_member(system.instance_id, RefusedMember::InstanceId)?,
            modified_at: optional_member(system.modified_at, RefusedMember::ModifiedAt)?,
            observed_at: optional_member(system.observed_at, RefusedMember::ObservedAt)?,
            sub_attributes,
        })
    }
}

/// Decode an optional member: absent and `null` are no value.
///
/// # Errors
///
/// Returns [`NodeRefusal::Member`] when the member does not decode as a `T`,
/// and the node's [`CompactNode::Unreadable`].
pub(crate) fn optional_member<T: DeserializeOwned, N: CompactNode>(member: Option<&N>, name: RefusedMember) -> Result<Option<T>, NodeReadError<N::Unreadable>> {
    let Some(member) = member else {
        return Ok(None);
    };
    decoded_optional(member)
        .map_err(NodeReadError::Unreadable)?
        .map_err(|source| NodeRefusal::Member { member: name, source }.into())
}
