//! Reading an attribute member and its instances (clauses 4.5, 4.5.5).
//!
//! An attribute member is an array of instance objects; the member set to the
//! NGSI-LD Null instead (`[{"@value": "urn:ngsi-ld:null"}]`) is the clause
//! 5.5.12 removal of the whole attribute. An instance's members are read in
//! whatever order they come; its `@type` is read first in every document the
//! broker or PostgreSQL writes, which is what lets a `GeoProperty`'s value be
//! read straight into its positions.

use crate::{
    codec::{
        carrier_decode::{CarrierContent, written_value},
        decode_context::DecodeContext,
        element::{ElementsSeed, TypeEntries},
        geometry_decode::GeoValueSeed,
        member_decode::{dataset_id, date_time, instance_id, malformed, references, seal_values, system_instant, unit_code, value_type},
        object_key::{ObjectKey, attribute_key},
        raw_scalar::RawScalar,
    },
    document::{attribute_member::AttributeMember, instance_document::InstanceDocument, slot::Slot, value_carrier::ValueCarrier, written_value::ValueSlot},
    expanded_value::ExpandedValue,
    member_role::{MemberRole, ProofSeal},
    member_shape::MemberShape,
    ngsi_ld_null::is_ngsi_ld_null,
    proof_seals::ProofSeals,
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    identifier::Identifier,
    reference::NgsiLdReference,
    time::{system_instant::SystemInstant, written_date_time::DateTimeText},
};
use cefact_units::UnitCode;
use indexmap::IndexMap;
use serde::{
    Deserializer,
    de::{DeserializeSeed, MapAccess, SeqAccess, Visitor},
};
use std::fmt;

/// Reads one attribute member: its instances, or its removal.
#[derive(Clone, Copy)]
pub(crate) struct AttributeMemberSeed<'c> {
    /// Where a refusal is recorded.
    pub(crate) context: &'c DecodeContext,
    /// The attribute.
    pub(crate) attribute: &'c AttributeName,
}

impl<'de> DeserializeSeed<'de> for AttributeMemberSeed<'_> {
    type Value = AttributeMember;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<AttributeMember, D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for AttributeMemberSeed<'_> {
    type Value = AttributeMember;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an array of attribute instances")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<AttributeMember, A::Error> {
        let mut instances = Vec::with_capacity(seq.size_hint().unwrap_or(1));
        let mut removals = 0_usize;
        while let Some(instance) = seq.next_element_seed(InstanceSeed {
            context: self.context,
            attribute: self.attribute,
        })? {
            match instance {
                InstanceDecoded::Instance(instance) => instances.push(*instance),
                InstanceDecoded::Null => removals += 1,
            }
        }
        match (removals, instances.is_empty()) {
            (0, false) => Ok(AttributeMember::Instances(instances)),
            (0, true) => Err(self.context.refuse(malformed(Some(self.attribute), MemberShape::Attribute))),
            (1, true) => Ok(AttributeMember::Remove),
            (_, _) => Err(self.context.refuse(EntityRefusal::NullBesideData {
                attribute: Some(self.attribute.clone()),
            })),
        }
    }
}

/// One element of an attribute member.
#[derive(Debug)]
pub(crate) enum InstanceDecoded {
    /// An instance.
    Instance(Box<InstanceDocument>),
    /// The NGSI-LD Null standing for the whole member.
    Null,
}

/// Reads one instance object.
#[derive(Clone, Copy)]
pub(crate) struct InstanceSeed<'c> {
    /// Where a refusal is recorded.
    pub(crate) context: &'c DecodeContext,
    /// The attribute the instance belongs to.
    pub(crate) attribute: &'c AttributeName,
}

impl<'de> DeserializeSeed<'de> for InstanceSeed<'_> {
    type Value = InstanceDecoded;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<InstanceDecoded, D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for InstanceSeed<'_> {
    type Value = InstanceDecoded;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an attribute instance")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<InstanceDecoded, A::Error> {
        let attribute = self.attribute;
        let refuse = |refusal: EntityRefusal| self.context.refuse::<A::Error>(refusal);
        let elements = ElementsSeed {
            context: self.context,
            owner: Some(attribute),
        };
        let mut parts = InstanceParts::default();
        while let Some(key) = map.next_key::<ObjectKey>()? {
            match key {
                ObjectKey::Type => parts.family = families(map.next_value::<TypeEntries>()?, attribute).map_err(refuse)?,
                ObjectKey::Value => {
                    if !map.next_value::<RawScalar<'de>>()?.text().is_some_and(is_ngsi_ld_null) {
                        return Err(refuse(malformed(Some(attribute), MemberShape::Attribute)));
                    }
                    parts.null_marker = true;
                }
                ObjectKey::Id => {
                    if !is_ngsi_ld_null(&map.next_value::<String>()?) {
                        return Err(refuse(malformed(Some(attribute), MemberShape::Attribute)));
                    }
                    parts.null_marker = true;
                }
                ObjectKey::Name(name) => {
                    let name = attribute_key(&name).map_err(refuse)?;
                    match MemberRole::of(name.as_str()) {
                        MemberRole::Carrier(carrier) => {
                            let content = if carrier == ValueCarrier::Value && parts.family == Some(AttributeType::GeoProperty) {
                                CarrierContent::Geometry(map.next_value_seed(GeoValueSeed {
                                    context: self.context,
                                    attribute,
                                })?)
                            } else {
                                CarrierContent::Elements(map.next_value_seed(elements)?)
                            };
                            parts.carriers.push((carrier, content));
                        }
                        MemberRole::DatasetId => parts.dataset_id = Some(dataset_id(map.next_value_seed(elements)?, attribute).map_err(refuse)?),
                        MemberRole::InstanceId => parts.instance_id = Some(instance_id(map.next_value_seed(elements)?, attribute).map_err(refuse)?),
                        MemberRole::ObservedAt => {
                            parts.observed_at = Some(date_time(map.next_value_seed(elements)?, Some(attribute), MemberShape::ObservedAt).map_err(refuse)?);
                        }
                        MemberRole::ExpiresAt => {
                            parts.expires_at = Some(date_time(map.next_value_seed(elements)?, Some(attribute), MemberShape::ExpiresAt).map_err(refuse)?);
                        }
                        MemberRole::DeletedAt => match date_time(map.next_value_seed(elements)?, Some(attribute), MemberShape::DeletedAt).map_err(refuse)? {
                            Slot::Set(deleted_at) => parts.deleted_at = Some(deleted_at),
                            Slot::Remove => return Err(refuse(EntityRefusal::ReservedNullValue { attribute: attribute.clone() })),
                        },
                        MemberRole::CreatedAt => {
                            parts.created_at = Some(system_instant(map.next_value_seed(elements)?, attribute, MemberShape::CreatedAt).map_err(refuse)?);
                        }
                        MemberRole::ModifiedAt => {
                            parts.modified_at = Some(system_instant(map.next_value_seed(elements)?, attribute, MemberShape::ModifiedAt).map_err(refuse)?);
                        }
                        MemberRole::UnitCode => parts.unit_code = Some(unit_code(map.next_value_seed(elements)?, attribute).map_err(refuse)?),
                        MemberRole::ValueType => parts.value_type = Some(value_type(map.next_value_seed(elements)?, attribute).map_err(refuse)?),
                        MemberRole::ObjectType => {
                            parts.object_types = Some(references(map.next_value_seed(elements)?, attribute, MemberShape::ObjectType).map_err(refuse)?);
                        }
                        MemberRole::Seal(seal) => {
                            let values = seal_values(map.next_value_seed(elements)?, attribute).map_err(refuse)?;
                            match seal {
                                ProofSeal::EntityId => parts.seal_entity_id = Some(values),
                                ProofSeal::EntityType => parts.seal_entity_type = Some(values),
                            }
                        }
                        MemberRole::OutputOnly | MemberRole::Scope => {
                            return Err(refuse(EntityRefusal::OutputOnlyMember {
                                attribute: attribute.clone(),
                                member: name,
                            }));
                        }
                        MemberRole::Attribute => {
                            let member = map.next_value_seed(AttributeMemberSeed {
                                context: self.context,
                                attribute: &name,
                            })?;
                            parts.sub_attributes.insert(name, member);
                        }
                    }
                }
                ObjectKey::InvalidName(source) => return Err(refuse(EntityRefusal::ExpandedMemberName(source))),
                ObjectKey::Language | ObjectKey::Direction | ObjectKey::List | ObjectKey::OtherKeyword => {
                    return Err(refuse(malformed(Some(attribute), MemberShape::Attribute)));
                }
            }
        }
        parts.finish(attribute).map_err(refuse)
    }
}

/// The families an instance's `@type` declares: none, or one recognised
/// family (clause 5.2.4).
fn families(types: TypeEntries, attribute: &AttributeName) -> Result<Option<AttributeType>, EntityRefusal> {
    let TypeEntries::Array(types) = types else {
        return Err(malformed(Some(attribute), MemberShape::InstanceType));
    };
    let mut family = None;
    for entry in types {
        let declared = AttributeType::from_expanded_iri(&entry).ok_or(EntityRefusal::UnsupportedAttributeType)?;
        match family {
            Some(held) if held != declared => return Err(EntityRefusal::SeveralFamilies { attribute: attribute.clone() }),
            Some(_) | None => family = Some(declared),
        }
    }
    Ok(family)
}

/// The members of one instance, gathered before it is assembled.
#[derive(Default)]
struct InstanceParts {
    /// The declared family.
    family: Option<AttributeType>,
    /// Whether the object carried the Null as a bare `@value` or `@id`.
    null_marker: bool,
    /// The value carriers, in order.
    carriers: Vec<(ValueCarrier, CarrierContent)>,
    /// `datasetId`.
    dataset_id: Option<Identifier>,
    /// `instanceId`.
    instance_id: Option<Identifier>,
    /// `observedAt`.
    observed_at: Option<Slot<DateTimeText>>,
    /// `expiresAt`.
    expires_at: Option<Slot<DateTimeText>>,
    /// `deletedAt`.
    deleted_at: Option<DateTimeText>,
    /// `createdAt`.
    created_at: Option<SystemInstant>,
    /// `modifiedAt`.
    modified_at: Option<SystemInstant>,
    /// `unitCode`.
    unit_code: Option<Slot<UnitCode>>,
    /// `valueType`.
    value_type: Option<Slot<NgsiLdReference>>,
    /// `objectType`.
    object_types: Option<Slot<Vec<NgsiLdReference>>>,
    /// `entityIdSealed`.
    seal_entity_id: Option<Vec<ExpandedValue>>,
    /// `entityTypeSealed`.
    seal_entity_type: Option<Vec<ExpandedValue>>,
    /// The sub-attributes.
    sub_attributes: IndexMap<AttributeName, AttributeMember>,
}

impl InstanceParts {
    /// Whether the object carried anything an instance carries.
    fn is_bare(&self) -> bool {
        self.family.is_none()
            && self.carriers.is_empty()
            && self.dataset_id.is_none()
            && self.instance_id.is_none()
            && self.observed_at.is_none()
            && self.expires_at.is_none()
            && self.deleted_at.is_none()
            && self.created_at.is_none()
            && self.modified_at.is_none()
            && self.unit_code.is_none()
            && self.value_type.is_none()
            && self.object_types.is_none()
            && self.seal_entity_id.is_none()
            && self.seal_entity_type.is_none()
            && self.sub_attributes.is_empty()
    }

    /// Assemble the instance, or recognise the member-level Null.
    fn finish(self, attribute: &AttributeName) -> Result<InstanceDecoded, EntityRefusal> {
        if self.null_marker {
            return if self.is_bare() {
                Ok(InstanceDecoded::Null)
            } else {
                Err(EntityRefusal::NullBesideData {
                    attribute: Some(attribute.clone()),
                })
            };
        }
        let mut carriers = self.carriers.into_iter();
        let value = match (carriers.next(), carriers.next()) {
            (None, _) => ValueSlot::Absent,
            (Some((carrier, content)), None) => {
                if let Some(family) = self.family
                    && ValueCarrier::of(family) != carrier
                {
                    return Err(EntityRefusal::member_forbidden(Some(attribute), Some(family), carrier.iri()));
                }
                written_value(carrier, content, self.family, attribute)?
            }
            (Some((first, _)), Some((second, _))) => {
                let stray = match self.family {
                    Some(family) if ValueCarrier::of(family) == second => first,
                    Some(_) | None => second,
                };
                return Err(EntityRefusal::member_forbidden(Some(attribute), self.family, stray.iri()));
            }
        };
        let document = InstanceDocument::builder()
            .family(self.family)
            .value(value)
            .dataset_id(self.dataset_id)
            .observed_at(self.observed_at)
            .expires_at(self.expires_at)
            .unit_code(self.unit_code)
            .value_type(self.value_type)
            .object_types(self.object_types)
            .created_at(self.created_at)
            .modified_at(self.modified_at)
            .deleted_at(self.deleted_at)
            .instance_id(self.instance_id)
            .proof_seals(ProofSeals::new(self.seal_entity_id, self.seal_entity_type))
            .sub_attributes(self.sub_attributes)
            .build();
        Ok(InstanceDecoded::Instance(Box::new(document)))
    }
}
