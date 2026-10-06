//! Holding a written document to the rules of the operation writing it
//! (clause 5.5.4).
//!
//! The decoder already refuses what no operation admits: a member not shaped
//! as its expanded form, an unusable `unitCode`, a geometry clause 4.7.1 does
//! not allow, a `datasetId` set to the NGSI-LD Null. What is left depends on
//! the document as a whole or on the operation: the instance identities of
//! clause 4.5.5.1, the value member a declared family requires (clause
//! 5.2.5), the members a family never carries (clauses 4.5.2.2 to 4.5.24.2),
//! and where the operation admits the NGSI-LD Null.
//!
//! The NGSI-LD Null placements are also held on their own, for a write whose
//! envelope rules are not the Core API's: a Temporal Evolution records a
//! deleted instance but may no more nest the Null under a key than a Core
//! write outside a merge.

use crate::{
    admission::{instance_completeness::InstanceCompleteness, null_sentinel_admission::NullSentinelAdmission},
    codec::geometry_decode::geo_value_of,
    document::{
        EntityDocument,
        attribute_member::AttributeMember,
        instance_document::InstanceDocument,
        slot::Slot,
        value_carrier::ValueCarrier,
        written_value::{ValueSlot, WrittenValue},
    },
    expanded_value::ExpandedValue,
    operation::instance_patch::written_family,
    refusal::EntityRefusal,
    value_mode::{Fragment, FragmentMember, FragmentText},
};
use andromeda_json_literal::{JsonLiteral, piece::JsonKind};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    identifier::Identifier,
    unit_code_admission::{UnitCodeAdmission, unit_code_admission},
};
use andromeda_ngsi_ld_representation::family_member_admission::MemberAdmission;
use std::collections::HashSet;

impl EntityDocument {
    /// Hold the document to the rules of an operation writing it under
    /// `admission`.
    ///
    /// # Errors
    ///
    /// Returns the [`EntityRefusal`] of the first rule the document breaks, in
    /// the order the attribute envelopes, the sub-attribute instance
    /// identities, the family members, the NGSI-LD Null placements, the scope
    /// and the geometries are checked.
    pub fn admit(&self, admission: NullSentinelAdmission) -> Result<(), EntityRefusal> {
        for (name, member) in self.attributes() {
            admit_envelopes(name, member, admission.value_omission())?;
        }
        for member in self.attributes().values() {
            member.instances().iter().try_for_each(admit_sub_attribute_identities)?;
        }
        for (name, member) in self.attributes() {
            member.instances().iter().try_for_each(|instance| admit_family_members(Some(name), instance))?;
        }
        for (name, member) in self.attributes() {
            member.instances().iter().try_for_each(|instance| admit_json_shapes(name, instance))?;
        }
        self.admit_null_placements(admission)?;
        for (name, member) in self.attributes() {
            member.instances().iter().try_for_each(|instance| admit_geometry(name, instance))?;
        }
        Ok(())
    }

    /// Hold the NGSI-LD Null placements of the document, and of nothing
    /// else, to the clause 5.5.4 prohibitions `admission` is not exempt from.
    ///
    /// # Errors
    ///
    /// Returns [`EntityRefusal::EntityExpiresAt`] or [`EntityRefusal::ScopeNull`]
    /// for an entity member deleted outside a fragment,
    /// [`EntityRefusal::ReservedNullValue`] for an attribute, instance or
    /// sub-attribute deleted outside a fragment, and
    /// [`EntityRefusal::NestedNullValue`] for the Null under a key of a Property
    /// value outside a merge fragment.
    pub fn admit_null_placements(&self, admission: NullSentinelAdmission) -> Result<(), EntityRefusal> {
        if admission.deletion_admission() == MemberAdmission::Forbidden && self.expires_at().is_some_and(Slot::is_remove) {
            return Err(EntityRefusal::EntityExpiresAt);
        }
        for (name, member) in self.attributes() {
            admit_member_nulls(name, member, admission)?;
        }
        if admission.deletion_admission() == MemberAdmission::Forbidden && self.scope().is_some_and(Slot::is_remove) {
            return Err(EntityRefusal::ScopeNull);
        }
        Ok(())
    }
}

impl InstanceDocument {
    /// Hold the bare attribute fragment of clauses 5.6.4 and 5.6.19, written
    /// for `attribute`, to the rules of an operation writing it under
    /// `admission`, as a `completeness` instance.
    ///
    /// # Errors
    ///
    /// Returns the [`EntityRefusal`] of the first rule the fragment breaks:
    /// the family's missing-value refusal comes first for a
    /// [`InstanceCompleteness::Complete`] fragment that names no value member.
    pub fn admit_fragment(&self, attribute: &AttributeName, admission: NullSentinelAdmission, completeness: InstanceCompleteness) -> Result<(), EntityRefusal> {
        if completeness == InstanceCompleteness::Complete && matches!(self.value(), ValueSlot::Absent) {
            return Err(EntityRefusal::missing_value(attribute, written_family(self, None)));
        }
        admit_sub_attribute_identities(self)?;
        for (name, member) in self.sub_attributes() {
            member
                .instances()
                .iter()
                .try_for_each(|instance| admit_envelope(name, instance, admission.value_omission()))?;
        }
        admit_family_members(None, self)?;
        admit_json_shapes(attribute, self)?;
        self.admit_null_placements(attribute, admission)?;
        admit_geometry(attribute, self)
    }

    /// Hold the NGSI-LD Null placements of the bare attribute fragment written
    /// for `attribute`, and of nothing else, to the clause 5.5.4 prohibitions
    /// `admission` is not exempt from.
    ///
    /// # Errors
    ///
    /// Returns [`EntityRefusal::ReservedNullValue`] for the instance or a
    /// sub-attribute deleted outside a fragment, and
    /// [`EntityRefusal::NestedNullValue`] for the Null under a key of a Property
    /// value outside a merge fragment.
    pub fn admit_null_placements(&self, attribute: &AttributeName, admission: NullSentinelAdmission) -> Result<(), EntityRefusal> {
        admit_instance_nulls(attribute, self, admission)
    }
}

/// The envelope rules of one attribute member: distinct instance identities
/// (clause 4.5.5.1), the value member a declared family requires (clause
/// 5.2.5) unless `value_omission` admits leaving it out, and no `unitCode` on
/// a unitless family (clauses 4.5.3, 4.5.18, 4.5.20, 4.5.24).
fn admit_envelopes(name: &AttributeName, member: &AttributeMember, value_omission: MemberAdmission) -> Result<(), EntityRefusal> {
    let instances = member.instances();
    admit_distinct_identities(name, instances)?;
    instances.iter().try_for_each(|instance| admit_envelope(name, instance, value_omission))
}

/// The written instances of the attribute `name` name distinct instances: no
/// two share a `datasetId` and at most one is the default instance (clause
/// 4.5.5.1). Every write lands its instances one by one on the held instance
/// with the same identity, so a repeated identity would silently land twice.
fn admit_distinct_identities(name: &AttributeName, instances: &[InstanceDocument]) -> Result<(), EntityRefusal> {
    let mut seen: HashSet<Option<&Identifier>> = HashSet::with_capacity(instances.len());
    instances.iter().try_for_each(|instance| {
        if seen.insert(instance.dataset_id()) {
            return Ok(());
        }
        // clone: the refusal outlives the borrowed request.
        Err(match instance.dataset_id() {
            Some(dataset_id) => EntityRefusal::RepeatedInstance {
                attribute: name.clone(),
                dataset_id: dataset_id.clone(),
            },
            None => EntityRefusal::MultipleDefaultInstances { attribute: name.clone() },
        })
    })
}

/// Clause 4.5.5.1 at every depth below `instance`: a sub-attribute is an
/// attribute of its instance (clause 4.5.2.2), so the instances of each of
/// its sub-attributes, and of theirs, name distinct instances too.
fn admit_sub_attribute_identities(instance: &InstanceDocument) -> Result<(), EntityRefusal> {
    instance.sub_attributes().iter().try_for_each(|(name, member)| {
        let instances = member.instances();
        admit_distinct_identities(name, instances)?;
        instances.iter().try_for_each(admit_sub_attribute_identities)
    })
}

/// The envelope rules of one instance, which may leave its value out where
/// `value_omission` admits it.
fn admit_envelope(name: &AttributeName, instance: &InstanceDocument, value_omission: MemberAdmission) -> Result<(), EntityRefusal> {
    let Some(family) = instance.family() else {
        return Ok(());
    };
    let missing_value = matches!(instance.value(), ValueSlot::Absent) && value_omission == MemberAdmission::Forbidden;
    match family {
        AttributeType::Property | AttributeType::GeoProperty if missing_value => {
            return Err(EntityRefusal::MissingValue {
                attribute_type: family,
                attribute: name.clone(),
            });
        }
        AttributeType::Relationship if missing_value => return Err(EntityRefusal::MissingObject { attribute: name.clone() }),
        AttributeType::Property
        | AttributeType::GeoProperty
        | AttributeType::Relationship
        | AttributeType::LanguageProperty
        | AttributeType::VocabProperty
        | AttributeType::JsonProperty
        | AttributeType::ListProperty
        | AttributeType::ListRelationship => {}
    }
    if instance.unit_code().is_some() && unit_code_admission(family) == UnitCodeAdmission::Forbidden {
        return Err(EntityRefusal::UnitCodeForbidden {
            family,
            attribute: name.clone(),
        });
    }
    Ok(())
}

/// No instance carries another family's value member, here or on any
/// sub-attribute below it (clauses 4.5.2.2 to 4.5.24.2).
fn admit_family_members(attribute: Option<&AttributeName>, instance: &InstanceDocument) -> Result<(), EntityRefusal> {
    if let Some(family) = instance.family() {
        let carrier = match instance.value() {
            ValueSlot::Set(value) => Some(value.carrier()),
            ValueSlot::Remove(carrier) => Some(*carrier),
            ValueSlot::Absent => None,
        };
        if let Some(carrier) = carrier.filter(|carrier| *carrier != ValueCarrier::of(family)) {
            return Err(EntityRefusal::member_forbidden(attribute, Some(family), carrier.iri()));
        }
    }
    for (name, member) in instance.sub_attributes() {
        member.instances().iter().try_for_each(|sub| admit_family_members(Some(name), sub))?;
    }
    Ok(())
}

/// The `json` of a `JsonProperty`, here or on any sub-attribute below it, is
/// a JSON object or an array of JSON objects (clause 4.5.24.2).
///
/// Clause 4.5.24.2 writes "a raw JSON object (or array of objects)" and
/// clause 4.5.24.3 "a raw JSON object", so a scalar, `null` or an array
/// holding anything but objects is no `JsonProperty` value. The NGSI-LD Null
/// that deletes the value is a deletion, not a value, and is held to the
/// clause 5.5.4 prohibitions instead.
fn admit_json_shapes(name: &AttributeName, instance: &InstanceDocument) -> Result<(), EntityRefusal> {
    if let ValueSlot::Set(WrittenValue::Json(literals)) = instance.value()
        && !literals.iter().all(is_an_object_or_objects)
    {
        return Err(EntityRefusal::JsonNotAnObject { attribute: name.clone() });
    }
    instance
        .sub_attributes()
        .iter()
        .try_for_each(|(sub_name, member)| member.instances().iter().try_for_each(|sub| admit_json_shapes(sub_name, sub)))
}

/// Whether a literal is a JSON object, or an array holding only JSON objects.
fn is_an_object_or_objects(literal: &JsonLiteral) -> bool {
    let piece = literal.piece();
    match piece.kind() {
        JsonKind::Object => true,
        JsonKind::Array => piece
            .elements()
            .is_some_and(|elements| elements.iter().all(|element| element.kind() == JsonKind::Object)),
        JsonKind::Null | JsonKind::Boolean | JsonKind::Number | JsonKind::String => false,
    }
}

/// The NGSI-LD Null placements of one attribute member and everything below
/// it.
fn admit_member_nulls(name: &AttributeName, member: &AttributeMember, admission: NullSentinelAdmission) -> Result<(), EntityRefusal> {
    match member {
        AttributeMember::Remove if admission.deletion_admission() == MemberAdmission::Forbidden => {
            Err(EntityRefusal::ReservedNullValue { attribute: name.clone() })
        }
        AttributeMember::Remove => Ok(()),
        AttributeMember::Instances(instances) => instances.iter().try_for_each(|instance| admit_instance_nulls(name, instance, admission)),
    }
}

/// The NGSI-LD Null placements of one instance and of its sub-attributes.
fn admit_instance_nulls(name: &AttributeName, instance: &InstanceDocument, admission: NullSentinelAdmission) -> Result<(), EntityRefusal> {
    if admission.deletion_admission() == MemberAdmission::Forbidden && deletes_a_member(instance) {
        return Err(EntityRefusal::ReservedNullValue { attribute: name.clone() });
    }
    if admission.nested_deletion_admission() == MemberAdmission::Forbidden && nests_a_deletion(instance) {
        return Err(EntityRefusal::NestedNullValue { attribute: name.clone() });
    }
    instance
        .sub_attributes()
        .iter()
        .try_for_each(|(sub_name, member)| admit_member_nulls(sub_name, member, admission))
}

/// Whether a written instance deletes itself or one of its members: the first
/// two prohibitions of clause 5.5.4.
fn deletes_a_member(instance: &InstanceDocument) -> bool {
    let deletes_a_language = matches!(
        instance.value(),
        ValueSlot::Set(WrittenValue::LanguageMap(entries)) if entries.entries().iter().any(|entry| matches!(entry.text(), FragmentText::Remove))
    );
    matches!(instance.value(), ValueSlot::Remove(_))
        || deletes_a_language
        || instance.observed_at().is_some_and(Slot::is_remove)
        || instance.expires_at().is_some_and(Slot::is_remove)
        || instance.unit_code().is_some_and(Slot::is_remove)
        || instance.value_type().is_some_and(Slot::is_remove)
        || instance.object_types().is_some_and(Slot::is_remove)
}

/// Whether the object on the right-hand side of a Property `value`, a JSON
/// literal anywhere inside it, or the JSON of a `JsonProperty`, holds the
/// NGSI-LD Null under one of its keys: the third prohibition of clause 5.5.4.
fn nests_a_deletion(instance: &InstanceDocument) -> bool {
    match instance.value() {
        ValueSlot::Set(WrittenValue::Value(values)) => values.iter().any(value_nests_a_deletion),
        ValueSlot::Set(WrittenValue::Json(json)) => json.iter().any(JsonLiteral::carries_null_sentinel_under_a_key),
        ValueSlot::Set(
            WrittenValue::Geometry(_)
            | WrittenValue::Object(_)
            | WrittenValue::LanguageMap(_)
            | WrittenValue::Vocab(_)
            | WrittenValue::ValueList(_)
            | WrittenValue::ObjectList(_),
        )
        | ValueSlot::Absent
        | ValueSlot::Remove(_) => false,
    }
}

/// Whether a member deletion stands anywhere inside `value`.
///
/// A JSON literal (a `@json`-typed value, or a member a context coerces to
/// `@json`) is a JSON object on the right-hand side of the value as much as a
/// structured node is, so the Null under one of its keys is the same
/// prohibition (clause 5.5.4).
fn value_nests_a_deletion(value: &ExpandedValue<Fragment>) -> bool {
    match value {
        ExpandedValue::Literal(_) => false,
        ExpandedValue::Json(json) => json.carries_null_sentinel_under_a_key(),
        ExpandedValue::List(items) => items.iter().any(value_nests_a_deletion),
        ExpandedValue::Node(node) => node.members().values().any(|member| match member {
            FragmentMember::Remove => true,
            FragmentMember::Values(values) => values.iter().any(value_nests_a_deletion),
        }),
    }
}

/// A `GeoProperty` instance whose `value` was written as plain values must
/// read as a geometry clause 4.7.1 allows.
fn admit_geometry(name: &AttributeName, instance: &InstanceDocument) -> Result<(), EntityRefusal> {
    match (instance.family(), instance.value()) {
        (Some(AttributeType::GeoProperty), ValueSlot::Set(WrittenValue::Value(values))) => geo_value_of(name, values.as_slice()).map(drop),
        (Some(_) | None, ValueSlot::Set(_) | ValueSlot::Absent | ValueSlot::Remove(_)) => Ok(()),
    }
}
