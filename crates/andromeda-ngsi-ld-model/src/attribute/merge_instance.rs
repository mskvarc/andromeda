//! One attribute instance of a merge fragment (ETSI GS CIM 009 clause 5.5.12).
//!
//! A merge fragment writes only what it changes: clause 5.5.12 merges it into
//! the target "up to an arbitrary depth", so an instance may leave out the
//! value member its family otherwise requires, and it removes a member it
//! writes as the NGSI-LD Null -- every member but `datasetId`, which "cannot be
//! deleted by setting it to the value urn:ngsi-ld:null". The family is always
//! known: an instance names it in `type` or implies it by its value member,
//! and anything else written where an attribute stands is a concise Property
//! or `GeoProperty` value (clause 4.5.2.3).
//!
//! The output-only members a client cannot write -- the `previous...` member,
//! the inline entities of a join, and the system timestamps of clause 4.8 --
//! are not read.

use crate::{
    attribute::{
        geo_property_value::GeoPropertyValue,
        instance_member::{InstanceMembers, ReadError, optional_dataset_member, optional_member},
        instance_node::{geo_refusal, json, required, value_refusal},
        member_reading::MemberReading,
        merge_member::MergeMember,
        merge_value::MergeValue,
        nested_null::NestedMergeMember,
        one_or_many::OneOrMany,
        patched::{Patched, is_ngsi_ld_null, patched_instant_member, patched_member},
        value::NgsiLdValue,
    },
    node_refusal::{NodeReadError, NodeRefusal, RefusedMember, RefusedNode},
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, coerced_term::CoercedTerm, identifier::Identifier};
use andromeda_ngsi_ld_representation::compact_node::{CompactNode, NodeMembers};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use cefact_units::UnitCode;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Serialize, Serializer, ser::SerializeMap};
use std::str::FromStr;

/// The member an instance names its family in.
const TYPE_MEMBER: &str = "type";

/// How a merge fragment wrote an instance.
///
/// Table 6.5.3.4-1's `format=simplified` reads every member that names no
/// family as the bare value of the attribute the target holds, so a concise
/// instance keeps the JSON it was written as for that reading.
#[derive(Debug, Clone, PartialEq)]
pub enum Declaration {
    /// As an object naming its family in `type`.
    Typed,
    /// Concisely, as written: an object whose value member implies its family,
    /// or the bare value of a concise Property or `GeoProperty` (clauses
    /// 4.5.2.3 and 4.7.3).
    Concise(JsonLiteral),
}

/// One attribute instance of a merge fragment.
#[derive(Debug, Clone, PartialEq)]
pub struct MergeInstance {
    /// The family the instance names or implies; none for a bare Attribute
    /// Fragment that writes neither a `type` nor a value member (clauses
    /// 5.6.4 and 5.6.17 give it the family of the attribute it updates).
    pub family: Option<AttributeType>,
    /// How the fragment wrote the instance.
    pub declaration: Declaration,
    /// The family's value member, when written.
    pub value: Option<Patched<MergeValue>>,
    /// `unitCode`, a Property's own.
    pub unit_code: Option<Patched<UnitCode>>,
    /// `valueType`, the property families' own.
    pub value_type: Option<Patched<CoercedTerm>>,
    /// `objectType`, the relationship families' own.
    pub object_type: Option<Patched<OneOrMany<CoercedTerm>>>,
    /// The `entityIdSealed` of an `ngsildproof` Property (clause 4.5.2.2).
    pub entity_id_sealed: Option<JsonLiteral>,
    /// The `entityTypeSealed` of an `ngsildproof` Property (clause 4.5.2.2).
    pub entity_type_sealed: Option<JsonLiteral>,
    /// `datasetId`, which addresses the instance and is never removed.
    pub dataset_id: Option<Identifier>,
    /// `expiresAt` (clause 4.22).
    pub expires_at: Option<Patched<DateTime<Utc>>>,
    /// `instanceId`.
    pub instance_id: Option<Identifier>,
    /// `observedAt`.
    pub observed_at: Option<Patched<DateTime<Utc>>>,
    /// The sub-attributes, by name, in written order.
    pub sub_attributes: IndexMap<AttributeName, MergeMember>,
}

impl MergeInstance {
    /// The instance of `family` the members of a compact object written under
    /// the attribute name `name` are.
    ///
    /// # Errors
    ///
    /// Returns [`NodeReadError::Refused`] when the members are no instance of
    /// `family`, located at the member at fault, and
    /// [`NodeReadError::Unreadable`] when a node of them has no JSON form.
    pub(crate) fn from_members<'n, N: CompactNode<'n>>(
        name: &str,
        family: Option<AttributeType>,
        node: N,
        members: NodeMembers<'n, N>,
    ) -> Result<MergeInstance, ReadError<'n, N>> {
        let declaration = if members.contains(TYPE_MEMBER) { Declaration::Typed } else { concise(node)? };
        // An instance that names no family is read as the Property clause
        // 4.5.2.3 infers for it; it carries no value member to read as one.
        let reading = family.unwrap_or(AttributeType::Property);
        let members = InstanceMembers::sorted(name, reading, members)?;
        let value = match members.value {
            None => None,
            Some(node) if is_ngsi_ld_null(node) => Some(Patched::Null),
            Some(node) => Some(Patched::Set(MergeValue::from_node(reading, required(node, RefusedMember::Value(reading))?)?)),
        };
        let system = &members.system;
        let mut sub_attributes = IndexMap::with_capacity(members.sub_attributes.len());
        for (member, node) in &members.sub_attributes {
            let attribute_name =
                AttributeName::from_str(member).map_err(|source| ReadError::<N>::from(NodeRefusal::AttributeName(source)).within_member(member))?;
            let value = MergeMember::from_node(member, *node).map_err(|error| error.within_member(member))?;
            sub_attributes.insert(attribute_name, value);
        }
        Ok(MergeInstance {
            family,
            declaration,
            value,
            unit_code: patched_member(members.unit_code, RefusedMember::UnitCode)?,
            value_type: patched_member(members.value_type, RefusedMember::ValueType)?,
            object_type: patched_member(members.object_type, RefusedMember::ObjectType)?,
            entity_id_sealed: members.entity_id_sealed.map(|seal| json(seal, RefusedMember::EntityIdSealed)).transpose()?,
            entity_type_sealed: members.entity_type_sealed.map(|seal| json(seal, RefusedMember::EntityTypeSealed)).transpose()?,
            dataset_id: optional_dataset_member(system.dataset_id)?,
            expires_at: patched_instant_member(system.expires_at, RefusedMember::ExpiresAt)?,
            instance_id: optional_member(system.instance_id, RefusedMember::InstanceId)?,
            observed_at: patched_instant_member(system.observed_at, RefusedMember::ObservedAt)?,
            sub_attributes,
        })
    }

    /// The concise Property a bare value written where an attribute stands
    /// abbreviates (clause 4.5.2.3).
    ///
    /// # Errors
    ///
    /// Returns the refusal of the value and the node's
    /// [`CompactNode::Unreadable`].
    pub(crate) fn concise_property<'n, N: CompactNode<'n>>(node: N) -> Result<MergeInstance, ReadError<'n, N>> {
        let member = RefusedMember::Value(AttributeType::Property);
        let value = NgsiLdValue::from_node(node)
            .map_err(NodeReadError::Unreadable)?
            .map_err(|refusal| ReadError::<N>::from(value_refusal(member, refusal)))?;
        Ok(MergeInstance::of_value(concise(node)?, MergeValue::Property(value)))
    }

    /// The concise `GeoProperty` a bare geometry written where an attribute
    /// stands abbreviates (clause 4.7.3).
    ///
    /// # Errors
    ///
    /// Returns the refusal of the geometry and the node's
    /// [`CompactNode::Unreadable`].
    pub(crate) fn concise_geo_property<'n, N: CompactNode<'n>>(node: N) -> Result<MergeInstance, ReadError<'n, N>> {
        let member = RefusedMember::Value(AttributeType::GeoProperty);
        let value = GeoPropertyValue::from_node(node)
            .map_err(NodeReadError::Unreadable)?
            .map_err(|refusal| ReadError::<N>::from(geo_refusal(member, refusal)))?;
        Ok(MergeInstance::of_value(concise(node)?, MergeValue::GeoProperty(value)))
    }

    /// The instance of `family` whose value is `written`: how Table
    /// 6.5.3.4-1's `format=simplified` reads a member that names no family,
    /// for an attribute the target holds as a `family`.
    ///
    /// # Errors
    ///
    /// Returns the [`RefusedNode`] of a value `family` does not hold, located
    /// from the value.
    pub fn of_family_value(family: AttributeType, written: &JsonLiteral) -> Result<MergeInstance, RefusedNode> {
        match MergeValue::from_node(family, written.piece()) {
            Ok(value) => Ok(MergeInstance::of_value(Declaration::Typed, value)),
            Err(NodeReadError::Refused(refused)) => Err(refused),
            Err(NodeReadError::Unreadable(never)) => match never {},
        }
    }

    /// The instance writing `value` alone, of the family the value is one of.
    #[must_use]
    pub fn of_value(declaration: Declaration, value: MergeValue) -> MergeInstance {
        MergeInstance {
            family: Some(value.family()),
            declaration,
            value: Some(Patched::Set(value)),
            unit_code: None,
            value_type: None,
            object_type: None,
            entity_id_sealed: None,
            entity_type_sealed: None,
            dataset_id: None,
            expires_at: None,
            instance_id: None,
            observed_at: None,
            sub_attributes: IndexMap::new(),
        }
    }
}

// Hand-written: the instance is written as its family's normalized object,
// whose members depend on the family and on which of them the fragment wrote.
impl Serialize for MergeInstance {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        if let Some(family) = self.family {
            map.serialize_entry(TYPE_MEMBER, family.short_name())?;
            if let Some(value) = &self.value {
                map.serialize_entry(family.compact_value_key(), value)?;
            }
        }
        optional_entry(&mut map, compact_properties::UNIT_CODE, self.unit_code.as_ref())?;
        optional_entry(&mut map, compact_properties::VALUE_TYPE, self.value_type.as_ref())?;
        optional_entry(&mut map, compact_properties::OBJECT_TYPE, self.object_type.as_ref())?;
        optional_entry(&mut map, compact_properties::ENTITY_ID_SEALED, self.entity_id_sealed.as_ref())?;
        optional_entry(&mut map, compact_properties::ENTITY_TYPE_SEALED, self.entity_type_sealed.as_ref())?;
        optional_entry(&mut map, compact_properties::DATASET_ID, self.dataset_id.as_ref())?;
        optional_entry(&mut map, compact_properties::EXPIRES_AT, self.expires_at.as_ref())?;
        optional_entry(&mut map, compact_properties::INSTANCE_ID, self.instance_id.as_ref())?;
        optional_entry(&mut map, compact_properties::OBSERVED_AT, self.observed_at.as_ref())?;
        for (name, member) in &self.sub_attributes {
            map.serialize_entry(name.as_str(), &NestedMergeMember(member))?;
        }
        map.end()
    }
}

/// The declaration of a concise instance: the JSON it was written as.
fn concise<'n, N: CompactNode<'n>>(node: N) -> Result<Declaration, ReadError<'n, N>> {
    node.literal().map_err(NodeReadError::Unreadable)?.map(Declaration::Concise).map_err(|source| {
        NodeRefusal::Literal {
            member: RefusedMember::Value(AttributeType::Property),
            source,
        }
        .into()
    })
}

/// Write `value` under `key` when present.
fn optional_entry<M: SerializeMap, T: Serialize>(map: &mut M, key: &str, value: Option<&T>) -> Result<(), M::Error> {
    value.map_or(Ok(()), |value| map.serialize_entry(key, value))
}
