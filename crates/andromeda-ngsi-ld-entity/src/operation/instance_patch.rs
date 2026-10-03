//! Applying one written attribute instance to the instance an attribute
//! holds, or making a new instance of it (clauses 5.5.8, 5.5.12, 5.6.2,
//! 5.6.3, 5.6.4, 5.6.17, 5.6.19).
//!
//! The three depths are the three ways the specification lands a written
//! instance on a held one. Update Attributes, Append Attributes and Replace
//! Attribute replace the whole instance (clause 5.6.2, EXAMPLE 2 of clause
//! 5.5.8), so a member the write omits is dropped. Merge Entity merges to
//! arbitrary depth (clause 5.5.12). A partial attribute update replaces each
//! member it names wholesale and deletes each member it sets to the NGSI-LD
//! Null (clause 5.5.8), and may never change the family (clause 5.6.4.4).
//! The sub-attributes of a merged instance are merged instance by instance in
//! turn, and those of a partially updated one are replaced instance by
//! instance, each matched by its `datasetId` (clauses 4.5.5.1, 5.5.8, 5.5.12).

use crate::{
    date_time_text::DateTimeText,
    document::{instance_document::InstanceDocument, slot::Slot, value_carrier::ValueCarrier, written_value::ValueSlot},
    expanded_value::ExpandedValue,
    instance::Instance,
    operation::{
        family_write::{ValueDepth, apply_companion_members, fresh_family_value, write_family_value},
        mutation_error::MutationError,
        sub_attribute_patch::{new_sub_attributes, patch_sub_attributes},
    },
    proof_seals::ProofSeals,
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};

/// How deeply a written instance is applied to the instance it lands on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InstancePatchDepth {
    /// The written instance replaces the held one whole.
    ReplaceInstance,
    /// The written instance merges into the held one (clause 5.5.12).
    DeepMerge,
    /// Each member the written instance names replaces the held one, and the
    /// family stays (clauses 5.5.8, 5.6.4).
    PartialUpdate,
}

/// The family a written instance takes when applied over a held instance of
/// family `held` (`None` when nothing is held).
///
/// The declared family wins. A fragment declaring none keeps the held family
/// when it writes the member that family reads its value from, so a typeless
/// `value` written onto a `GeoProperty` is a geometry; otherwise the member it
/// writes names the family, and an instance writing none is a Property
/// (clause 4.5.2).
pub(crate) fn written_family(document: &InstanceDocument, held: Option<AttributeType>) -> AttributeType {
    if let Some(family) = document.family() {
        return family;
    }
    match (document.value(), held) {
        (ValueSlot::Set(written), Some(family)) if ValueCarrier::of(family) == written.carrier() => family,
        (ValueSlot::Set(written), Some(_) | None) => written.carrier().implied_family(),
        (ValueSlot::Absent | ValueSlot::Remove(_), Some(family)) => family,
        (ValueSlot::Absent | ValueSlot::Remove(_), None) => AttributeType::Property,
    }
}

/// A new instance made of a written one alone, nothing being held to merge
/// into: every deletion it carries has nothing to delete and is left out.
///
/// `observed_at` stands in for the written `observedAt` when the write
/// stamps a default one (Table 6.5.3.4-1).
///
/// # Errors
///
/// Returns the family's missing-value refusal when the instance carries no
/// value, [`EntityRefusal::ReservedNullValue`] when its value is the
/// family's deletion encoding, and the refusals of
/// [`apply_companion_members`] and of its sub-attributes.
pub(crate) fn new_instance(
    attribute: &AttributeName,
    document: &InstanceDocument,
    observed_at: Option<&Slot<DateTimeText>>,
) -> Result<Instance, MutationError> {
    let family = written_family(document, None);
    let written = match document.value() {
        ValueSlot::Set(written) => written,
        ValueSlot::Absent => return Err(MutationError::from(EntityRefusal::missing_value(attribute, family))),
        ValueSlot::Remove(_) => return Err(reserved_null(attribute)),
    };
    let mut value = fresh_family_value(attribute, family, written)?;
    apply_companion_members(attribute, &mut value, document)?;
    let mut instance = Instance::builder()
        .value(value)
        .sub_attributes(new_sub_attributes(document.sub_attributes())?)
        .build();
    apply_instance_members(&mut instance, document, observed_at);
    Ok(instance)
}

/// Apply a written instance to the instance `held`, in place.
///
/// Under [`InstancePatchDepth::DeepMerge`] and
/// [`InstancePatchDepth::ReplaceInstance`] a written instance of another
/// family replaces the held one whole, because the members of one family
/// have no meaning under another (clause 5.2).
///
/// # Errors
///
/// Returns [`MutationError::AttributeTypeMismatch`] when a partial update
/// names another family, [`EntityRefusal::ReservedNullValue`] when the value
/// is a deletion encoding (the caller deletes the instance instead), and the
/// refusals of [`new_instance`].
pub(crate) fn patch_instance(
    attribute: &AttributeName,
    held: &mut Instance,
    document: &InstanceDocument,
    depth: InstancePatchDepth,
    observed_at: Option<&Slot<DateTimeText>>,
) -> Result<(), MutationError> {
    let family = written_family(document, Some(held.family()));
    let (value_depth, sub_attribute_depth) = match depth {
        InstancePatchDepth::ReplaceInstance => {
            *held = new_instance(attribute, document, observed_at)?;
            return Ok(());
        }
        InstancePatchDepth::DeepMerge if family != held.family() => {
            *held = new_instance(attribute, document, observed_at)?;
            return Ok(());
        }
        InstancePatchDepth::PartialUpdate if family != held.family() => {
            return Err(MutationError::AttributeTypeMismatch {
                attribute: attribute.clone(), // clone: the error outlives the borrowed request.
                fragment_type: family,
                instance_type: held.family(),
            });
        }
        InstancePatchDepth::DeepMerge => (ValueDepth::Merge, InstancePatchDepth::DeepMerge),
        InstancePatchDepth::PartialUpdate => (ValueDepth::Replace, InstancePatchDepth::ReplaceInstance),
    };
    match document.value() {
        ValueSlot::Set(written) => write_family_value(attribute, held.value_mut(), written, value_depth)?,
        ValueSlot::Absent => {}
        ValueSlot::Remove(_) => return Err(reserved_null(attribute)),
    }
    apply_companion_members(attribute, held.value_mut(), document)?;
    apply_instance_members(held, document, observed_at);
    patch_sub_attributes(held.sub_attributes_mut(), document.sub_attributes(), sub_attribute_depth)
}

/// Apply the members every family shares: a set member is written, a member
/// set to the NGSI-LD Null is deleted, an omitted one is kept.
fn apply_instance_members(instance: &mut Instance, document: &InstanceDocument, observed_at: Option<&Slot<DateTimeText>>) {
    if let Some(slot) = observed_at.or_else(|| document.observed_at()) {
        instance.set_observed_at(slot.as_set().cloned()); // clone: the stored instance owns its copy of the borrowed fragment.
    }
    if let Some(slot) = document.expires_at() {
        instance.set_expires_at(slot.as_set().cloned()); // clone: the stored instance owns its copy of the borrowed fragment.
    }
    if let Some(created_at) = document.created_at() {
        instance.set_created_at(Some(created_at));
    }
    if let Some(modified_at) = document.modified_at() {
        instance.set_modified_at(Some(modified_at));
    }
    if let Some(instance_id) = document.instance_id() {
        instance.set_instance_id(Some(instance_id.clone())); // clone: the stored instance owns its copy of the borrowed fragment.
    }
    let written_seals = document.proof_seals();
    if !written_seals.is_empty() {
        let held_seals = instance.proof_seals();
        // clone: the stored instance owns its copy of whichever seal it keeps.
        let seals = ProofSeals::new(
            written_seals.entity_id().or_else(|| held_seals.entity_id()).map(<[ExpandedValue]>::to_vec),
            written_seals.entity_type().or_else(|| held_seals.entity_type()).map(<[ExpandedValue]>::to_vec),
        );
        instance.set_proof_seals(seals);
    }
}

/// The refusal of a write that stores the NGSI-LD Null as a value (clause
/// 5.5.4).
fn reserved_null(attribute: &AttributeName) -> MutationError {
    EntityRefusal::ReservedNullValue {
        attribute: attribute.clone(), // clone: the refusal outlives the borrowed request.
    }
    .into()
}

#[cfg(test)]
mod tests {
    use crate::{
        document::{
            instance_document::InstanceDocument,
            value_carrier::ValueCarrier,
            written_value::{ValueSlot, WrittenValue},
        },
        non_empty::NonEmpty,
        operation::{
            instance_patch::{new_instance, written_family},
            mutation_error::MutationError,
            speed_entity::{name, number},
        },
        refusal::EntityRefusal,
    };
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;

    /// A typeless `value` keeps a held `GeoProperty` a `GeoProperty`, names a
    /// Property where nothing is held, and the member a typeless fragment
    /// writes names its family (clause 4.5.2).
    #[test]
    fn a_typeless_fragment_takes_the_family_its_value_member_names() {
        let typeless_value = InstanceDocument::builder()
            .value(ValueSlot::Set(WrittenValue::Value(NonEmpty::one(number(1)))))
            .build();
        pretty_assertions::assert_eq!(written_family(&typeless_value, Some(AttributeType::GeoProperty)), AttributeType::GeoProperty);
        pretty_assertions::assert_eq!(written_family(&typeless_value, None), AttributeType::Property);
        pretty_assertions::assert_eq!(written_family(&typeless_value, Some(AttributeType::Relationship)), AttributeType::Property);
        let typeless_object = InstanceDocument::builder()
            .value(ValueSlot::Set(WrittenValue::Object(NonEmpty::one(
                "urn:ngsi-ld:X:1".parse().expect("a valid URI"),
            ))))
            .build();
        pretty_assertions::assert_eq!(written_family(&typeless_object, Some(AttributeType::Property)), AttributeType::Relationship);
        pretty_assertions::assert_eq!(
            written_family(&InstanceDocument::builder().build(), Some(AttributeType::VocabProperty)),
            AttributeType::VocabProperty
        );
    }

    /// A deletion encoding cannot become an instance: state holds no NGSI-LD
    /// Null (clause 5.5.4).
    #[test]
    fn a_deletion_encoding_cannot_become_an_instance() {
        let deletion = InstanceDocument::builder().value(ValueSlot::Remove(ValueCarrier::Value)).build();
        let refused = new_instance(&name("https://example.org/p"), &deletion, None).expect_err("a deletion");
        assert!(
            matches!(refused, MutationError::Refused(ref refusal) if matches!(**refusal, EntityRefusal::ReservedNullValue { .. })),
            "{refused:?}"
        );
    }
}
