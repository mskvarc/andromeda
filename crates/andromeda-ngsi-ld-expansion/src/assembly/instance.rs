//! Reading one expanded attribute instance as a written instance.
//!
//! The members of an instance are told apart by their expanded IRI: the family
//! value member, the members every family shares (clause 5.2.5 and its
//! siblings), the two data-integrity seals of clause 4.5.2.2, the output-only
//! members a write never stores, and every other member, which is a
//! sub-attribute.

use crate::{
    assembly::{
        attribute_member,
        expanded_value::expanded_values,
        member_value::{dataset_id, date_time, date_time_slot, instance_id, malformed, reference_slot, references_slot, system_instant, unit_code_slot},
        written_value::value_slot,
    },
    expanded_tree::{ExpandedItem, ExpandedNode},
};
use andromeda_ngsi_ld_entity::{
    date_time_text::DateTimeText,
    document::{attribute_member::AttributeMember, instance_document::InstanceDocument, slot::Slot, value_carrier::ValueCarrier, written_value::ValueSlot},
    expanded_value::ExpandedValue,
    member_shape::MemberShape,
    proof_seals::ProofSeals,
    refusal::EntityRefusal,
    system_instant::SystemInstant,
    unit_code::UnitCode,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier, reference::NgsiLdReference};
use andromeda_ngsi_ld_vocabulary::{compact_to_expanded, expanded::properties};
use indexmap::IndexMap;

/// Read one expanded instance of `attribute`.
///
/// # Errors
///
/// Returns the refusal of the first member that is not what its expanded IRI
/// names.
pub fn instance_document(attribute: &AttributeName, node: ExpandedNode) -> Result<InstanceDocument, EntityRefusal> {
    if node.id.is_some() {
        return Err(malformed(attribute, MemberShape::Attribute));
    }
    let family = family(attribute, &node.types)?;
    let mut parts = InstanceParts::default();
    for (name, items) in node.members {
        parts.read(attribute, family, &name, items)?;
    }
    Ok(InstanceDocument::builder()
        .family(family)
        .dataset_id(parts.dataset_id)
        .value(parts.value.unwrap_or(ValueSlot::Absent))
        .observed_at(parts.observed_at)
        .expires_at(parts.expires_at)
        .unit_code(parts.unit_code)
        .value_type(parts.value_type)
        .object_types(parts.object_types)
        .created_at(parts.created_at)
        .modified_at(parts.modified_at)
        .deleted_at(parts.deleted_at)
        .instance_id(parts.instance_id)
        .proof_seals(ProofSeals::new(parts.entity_id_sealed, parts.entity_type_sealed))
        .sub_attributes(parts.sub_attributes)
        .build())
}

/// The family an instance declares by its `@type`, if it declares one.
fn family(attribute: &AttributeName, types: &[String]) -> Result<Option<AttributeType>, EntityRefusal> {
    let mut declared = None;
    for type_iri in types {
        let family = AttributeType::from_expanded_iri(type_iri).ok_or(EntityRefusal::UnsupportedAttributeType)?;
        match declared {
            Some(previous) if previous != family => {
                return Err(EntityRefusal::SeveralFamilies {
                    attribute: attribute.clone(), // clone: the refusal names the attribute the caller keeps
                });
            }
            Some(_) | None => declared = Some(family),
        }
    }
    Ok(declared)
}

/// The members of one instance, read one at a time.
#[derive(Default)]
struct InstanceParts {
    /// The value member it carries, when it carries one.
    carrier: Option<ValueCarrier>,
    /// The value.
    value: Option<ValueSlot>,
    /// `datasetId`.
    dataset_id: Option<Identifier>,
    /// `observedAt`.
    observed_at: Option<Slot<DateTimeText>>,
    /// `expiresAt`.
    expires_at: Option<Slot<DateTimeText>>,
    /// `unitCode`.
    unit_code: Option<Slot<UnitCode>>,
    /// `valueType`.
    value_type: Option<Slot<NgsiLdReference>>,
    /// `objectType`.
    object_types: Option<Slot<Vec<NgsiLdReference>>>,
    /// `createdAt`.
    created_at: Option<SystemInstant>,
    /// `modifiedAt`.
    modified_at: Option<SystemInstant>,
    /// `deletedAt`.
    deleted_at: Option<DateTimeText>,
    /// `instanceId`.
    instance_id: Option<Identifier>,
    /// `entityIdSealed`.
    entity_id_sealed: Option<Vec<ExpandedValue>>,
    /// `entityTypeSealed`.
    entity_type_sealed: Option<Vec<ExpandedValue>>,
    /// The sub-attributes.
    sub_attributes: IndexMap<AttributeName, AttributeMember>,
}

impl InstanceParts {
    /// Read the member `name` of an instance of `attribute`.
    fn read(&mut self, attribute: &AttributeName, family: Option<AttributeType>, name: &str, items: Vec<ExpandedItem>) -> Result<(), EntityRefusal> {
        let iri = name;
        if let Some(carrier) = ValueCarrier::from_iri(iri) {
            if self.carrier.is_some() {
                return Err(EntityRefusal::MemberForbidden {
                    attribute: Some(attribute.clone()), // clone: the refusal names the attribute the caller keeps
                    family,
                    member: name.parse::<AttributeName>().map_err(EntityRefusal::ExpandedAttributeName)?,
                });
            }
            self.carrier = Some(carrier);
            self.value = Some(value_slot(attribute, family, carrier, items)?);
        } else if iri == properties::OBSERVED_AT.as_str() {
            self.observed_at = Some(date_time_slot(&items, attribute, MemberShape::ObservedAt)?);
        } else if iri == properties::EXPIRES_AT.as_str() {
            self.expires_at = Some(date_time_slot(&items, attribute, MemberShape::ExpiresAt)?);
        } else if iri == properties::UNIT_CODE.as_str() {
            self.unit_code = Some(unit_code_slot(&items, attribute)?);
        } else if iri == properties::VALUE_TYPE.as_str() {
            self.value_type = Some(reference_slot(&items, attribute, MemberShape::ValueType)?);
        } else if iri == properties::OBJECT_TYPE.as_str() {
            self.object_types = Some(references_slot(&items, attribute, MemberShape::ObjectType)?);
        } else if iri == properties::DATASET_ID.as_str() {
            self.dataset_id = Some(dataset_id(&items, attribute)?);
        } else if iri == properties::INSTANCE_ID.as_str() {
            self.instance_id = Some(instance_id(&items, attribute)?);
        } else if iri == properties::CREATED_AT.as_str() {
            self.created_at = Some(system_instant(&items, attribute, MemberShape::CreatedAt)?);
        } else if iri == properties::MODIFIED_AT.as_str() {
            self.modified_at = Some(system_instant(&items, attribute, MemberShape::ModifiedAt)?);
        } else if iri == properties::DELETED_AT.as_str() {
            self.deleted_at = Some(date_time(&items, attribute, MemberShape::DeletedAt)?);
        } else if iri == properties::ENTITY_ID_SEALED.as_str() {
            self.entity_id_sealed = Some(expanded_values(attribute, items)?);
        } else if iri == properties::ENTITY_TYPE_SEALED.as_str() {
            self.entity_type_sealed = Some(expanded_values(attribute, items)?);
        } else {
            let member = name.parse::<AttributeName>().map_err(EntityRefusal::ExpandedAttributeName)?;
            if is_output_only(iri) {
                return Err(EntityRefusal::OutputOnlyMember {
                    attribute: attribute.clone(), // clone: the refusal names the attribute the caller keeps
                    member,
                });
            }
            let written = attribute_member(&member, items)?;
            self.sub_attributes.insert(member, written);
        }
        Ok(())
    }
}

/// Whether a member is one NGSI-LD writes only in responses or in the
/// Temporal API, never in an attribute instance: a family's `previous...`
/// member (clause 5.2.5 and its siblings), an inline join (`entity`,
/// `entityList`), a member of the simplified temporal representation
/// (`values`, `objects`, ...) or `scope`, which
/// belongs to the entity (clause 4.18).
fn is_output_only(iri: &str) -> bool {
    [properties::ENTITY, properties::ENTITY_LIST, properties::SCOPE]
        .iter()
        .any(|member| member.as_str() == iri)
        || AttributeType::ALL
            .iter()
            .any(|family| compact_to_expanded(family.previous_key()) == Some(iri) || family.expanded_temporal_values_key_str() == iri)
}
