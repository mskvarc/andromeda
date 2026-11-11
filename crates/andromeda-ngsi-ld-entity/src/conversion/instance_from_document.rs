//! Turning a written attribute instance into a stored one (clauses 4.5,
//! 5.6.1, 5.6.18, 5.6.19).
//!
//! A stored instance has exactly one family, the value its family requires,
//! only the members its family defines, and no NGSI-LD Null anywhere. A
//! written instance that declares no family is read as its value member names
//! it, and as a Property when it names none (clause 4.5.2). The written
//! members are moved, never copied. A written `instanceId` is dropped: Table
//! 5.2.5-2 makes it output-only, and clause 5.2.5 has the broker ignore one a
//! producer provides, so the Temporal Evolution instance the write adds is
//! given an identity of the broker's own (clause 4.5.7).

use crate::{
    attribute_instances::AttributeInstances,
    attributes::Attributes,
    codec::geometry_decode::{GeoDecoded, geo_value_of},
    conversion::stored_value::into_stored_values,
    document::{
        attribute_member::AttributeMember,
        instance_document::{InstanceDocument, InstanceDocumentParts},
        slot::Slot,
        value_carrier::ValueCarrier,
        written_value::{ValueSlot, WrittenValue},
    },
    family_value::FamilyValue,
    instance::Instance,
    member_role::MemberRole,
    member_shape::MemberShape,
    proof::{attribute_proof::names_a_seal, proof_fault::ProofFault},
    refusal::EntityRefusal,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    identifier::Identifier,
    reference::NgsiLdReference,
    unit_code_admission::{UnitCodeAdmission, unit_code_admission},
};
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, expanded::properties};
use cefact_units::UnitCode;
use indexmap::IndexMap;

impl Instance {
    /// The stored instance a written one makes, with its `datasetId`.
    ///
    /// # Errors
    ///
    /// Returns the [`EntityRefusal`] a create or a replacement answers: a
    /// missing value, a member the family never carries, a forbidden
    /// `unitCode`, the NGSI-LD Null anywhere, a repeated sub-attribute
    /// instance.
    pub fn try_from_document(attribute: &AttributeName, document: InstanceDocument) -> Result<(Option<Identifier>, Instance), EntityRefusal> {
        let family = document.implied_family().unwrap_or(AttributeType::Property);
        let parts = document.into_parts();
        reject_nulls(attribute, &parts)?;
        let envelope = Envelope {
            unit_code: parts.unit_code.and_then(Slot::into_set),
            value_type: parts.value_type.and_then(Slot::into_set),
            object_types: parts.object_types.and_then(Slot::into_set).unwrap_or_default(),
        };
        if envelope.unit_code.is_some() && unit_code_admission(family) == UnitCodeAdmission::Forbidden {
            return Err(EntityRefusal::UnitCodeForbidden {
                family,
                attribute: attribute.clone(),
            });
        }
        if envelope.value_type.is_some() && !family.defines_member(compact_properties::VALUE_TYPE) {
            return Err(EntityRefusal::member_forbidden(Some(attribute), Some(family), properties::VALUE_TYPE));
        }
        if !envelope.object_types.is_empty() && !family.defines_member(compact_properties::OBJECT_TYPE) {
            return Err(EntityRefusal::member_forbidden(Some(attribute), Some(family), properties::OBJECT_TYPE));
        }
        if parts.deleted_at.is_some() {
            return Err(EntityRefusal::member_forbidden(Some(attribute), Some(family), properties::DELETED_AT));
        }
        let sealing = names_a_seal(attribute);
        if !parts.proof_seals.is_empty() && !sealing {
            return Err(EntityRefusal::member_forbidden(Some(attribute), Some(family), properties::ENTITY_ID_SEALED));
        }
        if sealing && family != AttributeType::Property {
            return Err(EntityRefusal::proof(attribute, ProofFault::NotAProperty(family)));
        }
        let mut value = family_value(attribute, family, parts.value, envelope)?;
        if sealing {
            let seals = parts.proof_seals.complete().map_err(|fault| EntityRefusal::proof(attribute, fault))?;
            value = value.sealed(seals).map_err(|fault| EntityRefusal::proof(attribute, fault))?;
        }
        let instance = Instance::builder()
            .value(value)
            .observed_at(parts.observed_at.and_then(Slot::into_set))
            .expires_at(parts.expires_at.and_then(Slot::into_set))
            .created_at(parts.created_at)
            .modified_at(parts.modified_at)
            .sub_attributes(Attributes::try_from_members(parts.sub_attributes)?)
            .build();
        Ok((parts.dataset_id, instance))
    }
}

/// The members only some families define.
struct Envelope {
    /// `unitCode`.
    unit_code: Option<UnitCode>,
    /// `valueType`.
    value_type: Option<NgsiLdReference>,
    /// `objectType`.
    object_types: Vec<NgsiLdReference>,
}

/// Refuse a written instance deleting any of its members: state holds no
/// NGSI-LD Null (clause 5.5.4).
fn reject_nulls(attribute: &AttributeName, parts: &InstanceDocumentParts) -> Result<(), EntityRefusal> {
    let deletes_a_member = matches!(parts.value, ValueSlot::Remove(_))
        || parts.observed_at.as_ref().is_some_and(Slot::is_remove)
        || parts.expires_at.as_ref().is_some_and(Slot::is_remove)
        || parts.unit_code.as_ref().is_some_and(Slot::is_remove)
        || parts.value_type.as_ref().is_some_and(Slot::is_remove)
        || parts.object_types.as_ref().is_some_and(Slot::is_remove);
    if deletes_a_member {
        return Err(EntityRefusal::ReservedNullValue { attribute: attribute.clone() });
    }
    Ok(())
}

/// The family value a written value makes under `family`.
fn family_value(attribute: &AttributeName, family: AttributeType, value: ValueSlot, envelope: Envelope) -> Result<FamilyValue, EntityRefusal> {
    let written = match value {
        ValueSlot::Set(written) => written,
        ValueSlot::Absent | ValueSlot::Remove(_) => return Err(EntityRefusal::missing_value(attribute, family)),
    };
    if written.carrier() != ValueCarrier::of(family) {
        return Err(EntityRefusal::member_forbidden(Some(attribute), Some(family), written.carrier().iri()));
    }
    let Envelope {
        unit_code,
        value_type,
        object_types,
    } = envelope;
    let nested_null = || EntityRefusal::NestedNullValue { attribute: attribute.clone() };
    let value = match written {
        WrittenValue::Value(values) if family == AttributeType::GeoProperty => match geo_value_of(attribute, values.as_slice())? {
            GeoDecoded::Value(geometry) => FamilyValue::GeoProperty {
                geometry,
                unit_code,
                value_type,
            },
            GeoDecoded::Null => return Err(EntityRefusal::ReservedNullValue { attribute: attribute.clone() }),
        },
        WrittenValue::Value(values) => FamilyValue::Property {
            values: values
                .into_iter()
                .map(|value| value.into_stored().ok_or_else(nested_null))
                .collect::<Result<_, _>>()?,
            unit_code,
            value_type,
        },
        WrittenValue::Geometry(geometry) if family == AttributeType::GeoProperty => FamilyValue::GeoProperty {
            geometry,
            unit_code,
            value_type,
        },
        WrittenValue::Geometry(_) => {
            return Err(EntityRefusal::Malformed {
                attribute: Some(attribute.clone()),
                member: MemberShape::Value,
            });
        }
        WrittenValue::Object(objects) => FamilyValue::Relationship { objects, object_types },
        WrittenValue::LanguageMap(entries) => FamilyValue::LanguageProperty {
            entries: entries
                .into_stored()
                .ok_or_else(|| EntityRefusal::ReservedNullValue { attribute: attribute.clone() })?,
            value_type,
        },
        WrittenValue::Vocab(vocab) => FamilyValue::VocabProperty { vocab, value_type },
        WrittenValue::Json(json) => {
            if json.iter().any(JsonLiteral::is_null_sentinel) {
                return Err(EntityRefusal::ReservedNullValue { attribute: attribute.clone() });
            }
            if json.iter().any(JsonLiteral::carries_null_sentinel_under_a_key) {
                return Err(nested_null());
            }
            FamilyValue::JsonProperty { json, value_type }
        }
        WrittenValue::ValueList(items) => FamilyValue::ListProperty {
            items: into_stored_values(items).ok_or_else(nested_null)?,
            unit_code,
            value_type,
        },
        WrittenValue::ObjectList(objects) => FamilyValue::ListRelationship { objects, object_types },
    };
    Ok(value)
}

impl Attributes {
    /// The stored attributes written attribute members make.
    ///
    /// # Errors
    ///
    /// Returns the refusal of the first member that cannot be stored.
    pub fn try_from_members(members: IndexMap<AttributeName, AttributeMember>) -> Result<Attributes, EntityRefusal> {
        let mut attributes = Attributes::new();
        for (name, member) in members {
            if MemberRole::is_reserved(name.as_str()) {
                return Err(EntityRefusal::ReservedAttributeName { attribute: name });
            }
            let instances = match member {
                AttributeMember::Remove => return Err(EntityRefusal::ReservedNullValue { attribute: name }),
                AttributeMember::Instances(instances) => instances,
            };
            let converted = instances
                .into_iter()
                .map(|instance| Instance::try_from_document(&name, instance))
                .collect::<Result<Vec<_>, _>>()?;
            // clone: the refusal owns the attribute name it reports.
            let held = AttributeInstances::try_from_instances(converted).map_err(|repeated| EntityRefusal::repeated_instance(&name, repeated))?;
            if let Some(held) = held {
                attributes.insert(name, held);
            }
        }
        Ok(attributes)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        document::{
            attribute_member::AttributeMember,
            instance_document::InstanceDocument,
            written_value::{ValueSlot, WrittenValue},
        },
        instance::Instance,
        operation::speed_entity::{ACCURACY, SPEED, name, number},
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, time::system_instant::SystemInstant};
    use indexmap::IndexMap;

    /// The instant every written instance below claims.
    fn at() -> SystemInstant {
        "1900-01-01T00:00:00Z".parse().expect("RFC 3339")
    }

    /// A written Property instance claiming both system times.
    fn stamped(sub_attributes: IndexMap<AttributeName, AttributeMember>) -> InstanceDocument {
        InstanceDocument::builder()
            .family(Some(AttributeType::Property))
            .value(ValueSlot::Set(WrittenValue::Value(vec![number(1)])))
            .created_at(Some(at()))
            .modified_at(Some(at()))
            .sub_attributes(sub_attributes)
            .build()
    }

    /// One sub-attribute of `raw` holding `instance`.
    fn sub_attribute(raw: &str, instance: InstanceDocument) -> IndexMap<AttributeName, AttributeMember> {
        IndexMap::from([(name(raw), AttributeMember::Instances(vec![instance]))])
    }

    /// The system times of the default instance of `attribute` among the
    /// sub-attributes of `instance`.
    fn sub_attribute_times<'a>(instance: &'a Instance, attribute: &str) -> (&'a Instance, (Option<SystemInstant>, Option<SystemInstant>)) {
        let held = instance
            .sub_attributes()
            .get(attribute)
            .and_then(|instances| instances.get(None))
            .expect("the sub-attribute");
        (held, (held.created_at(), held.modified_at()))
    }

    /// A stored instance carries the times a write stamped at every depth
    /// (clause 4.8), so they read back at every depth; a time a client wrote
    /// is replaced when the write is stamped (clause 5.2.5).
    #[test]
    fn sub_attribute_system_times_are_kept_at_every_depth() {
        let method = "https://example.org/method";
        let written = stamped(sub_attribute(ACCURACY, stamped(sub_attribute(method, stamped(IndexMap::new())))));

        let (_, instance) = Instance::try_from_document(&name(SPEED), written).expect("a valid instance");

        pretty_assertions::assert_eq!((instance.created_at(), instance.modified_at()), (Some(at()), Some(at())));
        let (accuracy, accuracy_times) = sub_attribute_times(&instance, ACCURACY);
        pretty_assertions::assert_eq!(accuracy_times, (Some(at()), Some(at())));
        pretty_assertions::assert_eq!(sub_attribute_times(accuracy, method).1, (Some(at()), Some(at())));
    }

    /// Table 5.2.5-2 makes `instanceId` output-only and clause 5.2.5 has the
    /// broker ignore one a producer writes, so an instance written with one
    /// at any depth is stored as if written without.
    #[test]
    fn a_written_instance_id_is_ignored_at_every_depth() {
        let identified = |sub_attributes| {
            let mut document = stamped(sub_attributes);
            document.set_instance_id(Some("urn:ngsi-ld:Instance:client".parse().expect("a valid identifier")));
            document
        };
        let with = identified(sub_attribute(ACCURACY, identified(IndexMap::new())));
        let without = stamped(sub_attribute(ACCURACY, stamped(IndexMap::new())));

        pretty_assertions::assert_eq!(
            Instance::try_from_document(&name(SPEED), with).expect("a valid instance"),
            Instance::try_from_document(&name(SPEED), without).expect("a valid instance")
        );
    }
}
