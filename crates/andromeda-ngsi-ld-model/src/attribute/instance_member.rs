//! What each member of a compact attribute instance is to the instance's
//! family (ETSI GS CIM 009 clause 5.2 and Tables 5.2.5-1 to 5.2.38-1).
//!
//! An instance names its family in `type`, or implies it (clause 4.5.2.3 and
//! its siblings). Each family has a member holding its value and one holding
//! the previous value `showChanges` reports, and some have members of their
//! own: `unitCode` on a Property, a `GeoProperty` and a `ListProperty`,
//! `valueType` on the property families,
//! `objectType` and the inline `entity` / `entityList` of a clause 4.5.23 join
//! on the relationship families. The system members of clauses 4.5.5, 4.5.7,
//! 4.8 and 4.22 belong to every family, and every other member is a
//! sub-attribute (clause 4.5.6). What a member is depends on the family alone:
//! a `unitCode` beside a `ListProperty` value is a sub-attribute of that name,
//! exactly as the family's own members are in a decoded document.

use crate::{
    attribute::{common_fields::CommonAttributeFields, instance::AttributeValue, instance_node::required, member_reading::MemberReading},
    compact_member::decoded,
    node_refusal::{NodeReadError, NodeRefusal, RefusedMember},
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    identifier::Identifier,
    time::date_time::NgsiLdDateTime,
    unit_code_admission::{UnitCodeAdmission, unit_code_admission},
};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use andromeda_ngsi_ld_representation::{
    attribute_type_validation::AttributeNodeFault,
    compact_node::{CompactNode, NodeMembers, NodeShape},
    family_member_admission::{MemberAdmission, prohibited_member},
    proof_seal_admission::{ProofSealMember, proof_seal_admission},
};
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, keyword};
use cefact_units::UnitCode;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::de::DeserializeOwned;
use std::{borrow::Cow, str::FromStr};

/// The failure of reading a member of a node of type `N`.
pub(crate) type ReadError<'n, N> = NodeReadError<<N as CompactNode<'n>>::Unreadable>;

/// What one member of a compact instance is to its family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InstanceMember {
    /// `type`, which names the family.
    Family,
    /// The member holding the family's value.
    Value,
    /// The member holding the family's previous value.
    Previous,
    /// `unitCode`, the own member of a Property, a `GeoProperty` (clause
    /// 4.7.3) and a `ListProperty` (clause 4.5.21.2).
    UnitCode,
    /// `valueType`, the property families' own.
    ValueType,
    /// `objectType`, the relationship families' own.
    ObjectType,
    /// `entity` or `entityList`, the linked entities of a join.
    Inline,
    /// `entityIdSealed` of the `ngsildproof` Property (clause 4.5.2.2).
    EntityIdSealed,
    /// `entityTypeSealed` of the `ngsildproof` Property (clause 4.5.2.2).
    EntityTypeSealed,
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
    /// What the member named `name` is to `family`, in an instance of the
    /// attribute named `attribute`.
    ///
    /// # Errors
    ///
    /// Returns [`AttributeNodeFault::ProofSealOutsideNgsildproof`] when `name`
    /// spells a seal standing where clause 4.5.2.2 does not admit it: the seals
    /// are "non-reified subproperties" of the `ngsildproof` Property alone, so
    /// elsewhere one is neither a member of the instance nor a sub-attribute it
    /// could be read as. Returns
    /// [`AttributeNodeFault::ValueMemberOfAnotherFamily`] when `name` is the
    /// value member of another family, which the "Prohibited" list of the
    /// family's clause forbids, so it is no sub-attribute either.
    fn of(attribute: &str, family: AttributeType, name: &str) -> Result<InstanceMember, AttributeNodeFault> {
        if let Some(seal) = ProofSealMember::of(name) {
            return match (proof_seal_admission(attribute, family, name), seal) {
                (MemberAdmission::Admitted, ProofSealMember::EntityIdSealed) => Ok(InstanceMember::EntityIdSealed),
                (MemberAdmission::Admitted, ProofSealMember::EntityTypeSealed) => Ok(InstanceMember::EntityTypeSealed),
                (MemberAdmission::Forbidden, refused) => Err(AttributeNodeFault::ProofSealOutsideNgsildproof(refused)),
            };
        }
        if let Some(member) = prohibited_member(family, name) {
            return Err(AttributeNodeFault::ValueMemberOfAnotherFamily { family, member });
        }
        Ok(match (name, family) {
            (keyword::TYPE_ALIAS, _) => InstanceMember::Family,
            (own, family) if own == family.compact_value_key() => InstanceMember::Value,
            (own, family) if own == family.previous_key() => InstanceMember::Previous,
            (compact_properties::UNIT_CODE, family) if family.defines_member(name) => InstanceMember::UnitCode,
            (compact_properties::VALUE_TYPE, family) if family.defines_member(name) => InstanceMember::ValueType,
            (compact_properties::OBJECT_TYPE, family) if family.defines_member(name) => InstanceMember::ObjectType,
            (compact_properties::ENTITY | compact_properties::ENTITY_LIST, family) if family.defines_member(name) => InstanceMember::Inline,
            (compact_properties::CREATED_AT, _) => InstanceMember::CreatedAt,
            (compact_properties::DATASET_ID, _) => InstanceMember::DatasetId,
            (compact_properties::DELETED_AT, _) => InstanceMember::DeletedAt,
            (compact_properties::EXPIRES_AT, _) => InstanceMember::ExpiresAt,
            (compact_properties::INSTANCE_ID, _) => InstanceMember::InstanceId,
            (compact_properties::MODIFIED_AT, _) => InstanceMember::ModifiedAt,
            (compact_properties::OBSERVED_AT, _) => InstanceMember::ObservedAt,
            (_other, _) => InstanceMember::SubAttribute,
        })
    }
}

/// The members of one compact instance, by what its family makes of them.
#[derive(Debug)]
pub(crate) struct InstanceMembers<'n, N> {
    /// The family's value member.
    pub(crate) value: Option<N>,
    /// The family's previous-value member.
    pub(crate) previous: Option<N>,
    /// `unitCode`.
    pub(crate) unit_code: Option<N>,
    /// `valueType`.
    pub(crate) value_type: Option<N>,
    /// `objectType`.
    pub(crate) object_type: Option<N>,
    /// `entity` or `entityList`.
    pub(crate) inline: Option<N>,
    /// The `entityIdSealed` seal of an `ngsildproof` Property.
    pub(crate) entity_id_sealed: Option<N>,
    /// The `entityTypeSealed` seal of an `ngsildproof` Property.
    pub(crate) entity_type_sealed: Option<N>,
    /// The system members, by kind.
    pub(crate) system: SystemMembers<N>,
    /// The sub-attributes, by name, in written order.
    pub(crate) sub_attributes: Vec<(Cow<'n, str>, N)>,
}

/// The system members every family carries.
#[derive(Debug)]
pub(crate) struct SystemMembers<N> {
    /// `createdAt`.
    pub(crate) created_at: Option<N>,
    /// `datasetId`.
    pub(crate) dataset_id: Option<N>,
    /// `deletedAt`.
    pub(crate) deleted_at: Option<N>,
    /// `expiresAt`.
    pub(crate) expires_at: Option<N>,
    /// `instanceId`.
    pub(crate) instance_id: Option<N>,
    /// `modifiedAt`.
    pub(crate) modified_at: Option<N>,
    /// `observedAt`.
    pub(crate) observed_at: Option<N>,
}

impl<'n, N: CompactNode<'n>> InstanceMembers<'n, N> {
    /// Sort the members of an instance of `family` of the attribute named
    /// `attribute`; of a member written twice, the last counts, as a parsed
    /// object keeps it.
    ///
    /// # Errors
    ///
    /// Returns [`AttributeNodeFault::ProofSealOutsideNgsildproof`], located at
    /// the instance, for a seal clause 4.5.2.2 does not admit on it, and
    /// [`AttributeNodeFault::ValueMemberOfAnotherFamily`] for a value member
    /// the family's clause prohibits.
    pub(crate) fn sorted(attribute: &str, family: AttributeType, members: NodeMembers<'n, N>) -> Result<InstanceMembers<'n, N>, ReadError<'n, N>> {
        let mut sorted = InstanceMembers {
            value: None,
            previous: None,
            unit_code: None,
            value_type: None,
            object_type: None,
            inline: None,
            entity_id_sealed: None,
            entity_type_sealed: None,
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
            if name == compact_properties::UNIT_CODE && unit_code_admission(family) == UnitCodeAdmission::Forbidden {
                return Err(NodeRefusal::UnitCodeForbidden {
                    attribute: attribute_name(attribute)?,
                    family,
                }
                .into());
            }
            let member_kind = InstanceMember::of(attribute, family, &name).map_err(|fault| ReadError::<N>::from(NodeRefusal::Shape(fault)))?;
            let slot = match member_kind {
                InstanceMember::Family => continue,
                InstanceMember::Value => &mut sorted.value,
                InstanceMember::Previous => &mut sorted.previous,
                InstanceMember::UnitCode => &mut sorted.unit_code,
                InstanceMember::ValueType => &mut sorted.value_type,
                InstanceMember::ObjectType => &mut sorted.object_type,
                InstanceMember::Inline => &mut sorted.inline,
                InstanceMember::EntityIdSealed => &mut sorted.entity_id_sealed,
                InstanceMember::EntityTypeSealed => &mut sorted.entity_type_sealed,
                InstanceMember::CreatedAt => &mut sorted.system.created_at,
                InstanceMember::DatasetId => &mut sorted.system.dataset_id,
                InstanceMember::DeletedAt => &mut sorted.system.deleted_at,
                InstanceMember::ExpiresAt => &mut sorted.system.expires_at,
                InstanceMember::InstanceId => &mut sorted.system.instance_id,
                InstanceMember::ModifiedAt => &mut sorted.system.modified_at,
                InstanceMember::ObservedAt => &mut sorted.system.observed_at,
                InstanceMember::SubAttribute => {
                    sorted.sub_attributes.retain(|(written, _)| *written != name);
                    sorted.sub_attributes.push((name, member));
                    continue;
                }
            };
            *slot = Some(member);
        }
        Ok(sorted)
    }

    /// The members every family shares: the system members and the
    /// sub-attributes.
    ///
    /// # Errors
    ///
    /// Returns [`NodeRefusal::Member`] for a system member that does not
    /// decode, [`NodeRefusal::AttributeName`] for a sub-attribute named by no
    /// attribute name, the refusal of a sub-attribute, and the node's
    /// [`CompactNode::Unreadable`].
    pub(crate) fn common(&self) -> Result<CommonAttributeFields, ReadError<'n, N>> {
        let system = &self.system;
        let mut sub_attributes = IndexMap::with_capacity(self.sub_attributes.len());
        for (name, member) in &self.sub_attributes {
            let attribute_name =
                AttributeName::from_str(name).map_err(|source| ReadError::<N>::from(NodeRefusal::AttributeName(source)).within_member(name))?;
            let value = AttributeValue::from_node(name, *member).map_err(|error| error.within_member(name))?;
            sub_attributes.insert(attribute_name, value);
        }
        Ok(CommonAttributeFields {
            created_at: optional_instant_member(system.created_at, RefusedMember::CreatedAt)?,
            dataset_id: optional_dataset_member(system.dataset_id)?,
            deleted_at: optional_instant_member(system.deleted_at, RefusedMember::DeletedAt)?,
            expires_at: optional_instant_member(system.expires_at, RefusedMember::ExpiresAt)?,
            instance_id: optional_member(system.instance_id, RefusedMember::InstanceId)?,
            modified_at: optional_instant_member(system.modified_at, RefusedMember::ModifiedAt)?,
            observed_at: optional_instant_member(system.observed_at, RefusedMember::ObservedAt)?,
            sub_attributes,
        })
    }
}

/// The attribute name an instance is written under.
///
/// # Errors
///
/// Returns [`NodeRefusal::AttributeName`] for a name that is no attribute name.
fn attribute_name(attribute: &str) -> Result<AttributeName, NodeRefusal> {
    AttributeName::from_str(attribute).map_err(NodeRefusal::AttributeName)
}

/// Decode the `unitCode` of an instance of the attribute named `attribute`:
/// absent is no unit, a JSON `null` is no value of any member (clause 5.2.1),
/// and one that is no UN/CEFACT common code refuses the instance (Table
/// 5.2.5-1).
///
/// # Errors
///
/// Returns [`NodeRefusal::Null`] for a `null` member,
/// [`NodeRefusal::UnitCodeUnusable`] for a member that does not decode as a
/// common code, and the node's [`CompactNode::Unreadable`].
pub(crate) fn unit_code_member<'n, N: CompactNode<'n>>(attribute: &str, member: Option<N>) -> Result<Option<UnitCode>, ReadError<'n, N>> {
    let Some(member) = member else {
        return Ok(None);
    };
    let member = required(member, RefusedMember::UnitCode)?;
    match decoded::<UnitCode, N>(member).map_err(NodeReadError::Unreadable)? {
        Ok(unit_code) => Ok(Some(unit_code)),
        Err(source) => Err(refused_member::<N>(
            RefusedMember::UnitCode,
            NodeRefusal::UnitCodeUnusable {
                attribute: attribute_name(attribute)?,
                source,
            },
        )),
    }
}

/// The refusal of the member `member`, located at it.
pub(crate) fn refused_member<'n, N: CompactNode<'n>>(member: RefusedMember, refusal: NodeRefusal) -> ReadError<'n, N> {
    ReadError::<N>::from(refusal).within_member(&member.to_string())
}

/// Decode a member that is present as a `T`, the refusal left for the caller
/// to locate: a JSON `null` is no value of any member (clause 5.2.1).
///
/// # Errors
///
/// Returns [`NodeRefusal::Null`] for a `null` member, [`NodeRefusal::Member`]
/// when the member does not decode as a `T`, and the node's
/// [`CompactNode::Unreadable`].
pub(crate) fn present_member<'n, T: DeserializeOwned, N: CompactNode<'n>>(member: N, name: RefusedMember) -> Result<T, ReadError<'n, N>> {
    // The shape of an array or an object holds a node per item, so it is
    // dropped before the member is decoded: held across the decode, it would
    // add the member's size to the decoded value's.
    if matches!(member.shape(), NodeShape::Null) {
        return Err(NodeRefusal::Null.into());
    }
    decoded(member)
        .map_err(NodeReadError::Unreadable)?
        .map_err(|source| NodeRefusal::Member { member: name, source }.into())
}

/// Decode an optional member, located at it: absent is no value.
///
/// # Errors
///
/// As [`present_member`].
pub(crate) fn optional_member<'n, T: DeserializeOwned, N: CompactNode<'n>>(member: Option<N>, name: RefusedMember) -> Result<Option<T>, ReadError<'n, N>> {
    member
        .map(|member| present_member(member, name).map_err(|error| error.within_member(&name.to_string())))
        .transpose()
}

/// Read an optional instant member (`observedAt`, `expiresAt`, a system
/// time) by the clause 4.6.3 `DateTime` rules.
///
/// # Errors
///
/// Returns [`NodeRefusal::Member`] when the member is not a clause 4.6.3
/// `DateTime`, and the node's [`CompactNode::Unreadable`].
pub(crate) fn optional_instant_member<'n, N: CompactNode<'n>>(member: Option<N>, name: RefusedMember) -> Result<Option<DateTime<Utc>>, ReadError<'n, N>> {
    Ok(optional_member::<NgsiLdDateTime, N>(member, name)?.map(NgsiLdDateTime::get))
}

/// Read the optional `datasetId` of an instance (clause 4.5.5).
///
/// "If no datasetId is provided, or "datasetId": "@none" is supplied, it is
/// considered as the default Attribute instance", so the keyword reads as no
/// `datasetId` at all and the instance is the one an absent member makes.
///
/// # Errors
///
/// Returns [`NodeRefusal::Member`] when the member is neither the `@none`
/// keyword nor a URI, and the node's [`CompactNode::Unreadable`].
pub(crate) fn optional_dataset_member<'n, N: CompactNode<'n>>(member: Option<N>) -> Result<Option<Identifier>, ReadError<'n, N>> {
    Ok(optional_member::<DatasetSelector, N>(member, RefusedMember::DatasetId)?.and_then(DatasetSelector::into_named))
}
