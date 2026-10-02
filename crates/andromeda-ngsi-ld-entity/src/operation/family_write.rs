//! Writing a written value and the family's own members into an instance's
//! family value (clause 4.5).
//!
//! Each family reads its value out of one member and defines its own set of
//! companion members: `unitCode` only on the quantity families (clauses
//! 4.5.2.2, 4.7.3, 4.5.21.2), `objectType` only on the two relationship
//! families (clauses 4.5.3.2, 4.5.22.2) and `valueType` on every other one. A
//! written value under another family's member, or a companion member the
//! family never defines, cannot become state.

use crate::{
    codec::geometry_decode::{GeoDecoded, geo_value_of},
    document::{instance_document::InstanceDocument, slot::Slot, value_carrier::ValueCarrier, written_value::WrittenValue},
    expanded_value::ExpandedValue,
    family_value::FamilyValue,
    geo_value::GeoValue,
    json_literal::JsonLiteral,
    non_empty::{EmptyCollection, NonEmpty},
    operation::{
        mutation_error::MutationError,
        value_merge::{MemberMerge, merge_json, merge_language_entries, merge_values, settled_entries, settled_json, settled_values},
    },
    refusal::EntityRefusal,
    value_mode::Fragment,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
use andromeda_ngsi_ld_vocabulary::expanded::properties;

/// How a written value meets the value an instance already holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ValueDepth {
    /// The written value replaces the held one wholesale (clause 5.5.8).
    Replace,
    /// The written value merges into the held one (clause 5.5.12).
    Merge,
}

/// The family value `written` makes on its own, before any companion member.
///
/// # Errors
///
/// Returns the family's missing-value refusal when `written` is not under the
/// member `family` reads its value from, and the geometry fault when a
/// `GeoProperty` value is not a geometry clause 4.7.1 admits.
pub(crate) fn fresh_family_value(attribute: &AttributeName, family: AttributeType, written: &WrittenValue) -> Result<FamilyValue, MutationError> {
    if ValueCarrier::of(family) != written.carrier() {
        return Err(MutationError::from(EntityRefusal::missing_value(attribute, family)));
    }
    let missing = || MutationError::from(EntityRefusal::missing_value(attribute, family));
    Ok(match family {
        AttributeType::Property => {
            let WrittenValue::Value(values) = written else { return Err(missing()) };
            FamilyValue::Property {
                values: settled_non_empty(attribute, family, values.as_slice())?,
                unit_code: None,
                value_type: None,
            }
        }
        AttributeType::GeoProperty => FamilyValue::GeoProperty {
            geometry: written_geometry(attribute, written)?,
            unit_code: None,
            value_type: None,
        },
        AttributeType::Relationship => {
            let WrittenValue::Object(objects) = written else { return Err(missing()) };
            FamilyValue::Relationship {
                objects: objects.clone(), // clone: the stored instance owns its copy of the borrowed fragment.
                object_types: Vec::new(),
            }
        }
        AttributeType::LanguageProperty => {
            let WrittenValue::LanguageMap(entries) = written else { return Err(missing()) };
            FamilyValue::LanguageProperty {
                entries: settled_entries(entries),
                value_type: None,
            }
        }
        AttributeType::VocabProperty => {
            let WrittenValue::Vocab(vocab) = written else { return Err(missing()) };
            FamilyValue::VocabProperty {
                vocab: vocab.clone(), // clone: the stored instance owns its copy of the borrowed fragment.
                value_type: None,
            }
        }
        AttributeType::JsonProperty => {
            let WrittenValue::Json(json) = written else { return Err(missing()) };
            FamilyValue::JsonProperty {
                json: NonEmpty::try_from_vec(json.iter().map(settled_json).collect()).map_err(|_| missing())?,
                value_type: None,
            }
        }
        AttributeType::ListProperty => {
            let WrittenValue::ValueList(items) = written else { return Err(missing()) };
            FamilyValue::ListProperty {
                items: settled_values(items),
                unit_code: None,
                value_type: None,
            }
        }
        AttributeType::ListRelationship => {
            let WrittenValue::ObjectList(objects) = written else { return Err(missing()) };
            FamilyValue::ListRelationship {
                objects: objects.clone(), // clone: the stored instance owns its copy of the borrowed fragment.
                object_types: Vec::new(),
            }
        }
    })
}

/// Write `written` into the value `value` already holds, keeping the family
/// and every companion member.
///
/// # Errors
///
/// As [`fresh_family_value`].
pub(crate) fn write_family_value(attribute: &AttributeName, value: &mut FamilyValue, written: &WrittenValue, depth: ValueDepth) -> Result<(), MutationError> {
    let family = value.family();
    if ValueCarrier::of(family) != written.carrier() {
        return Err(MutationError::from(EntityRefusal::missing_value(attribute, family)));
    }
    let missing = || MutationError::from(EntityRefusal::missing_value(attribute, family));
    match value {
        FamilyValue::Property { values, .. } => {
            let WrittenValue::Value(written_values) = written else {
                return Err(missing());
            };
            let merged = match depth {
                ValueDepth::Merge => merge_values(values.as_mut_slice(), written_values.as_slice()),
                ValueDepth::Replace => MemberMerge::Replace,
            };
            if merged == MemberMerge::Replace {
                *values = settled_non_empty(attribute, family, written_values.as_slice())?;
            }
        }
        FamilyValue::GeoProperty { geometry, .. } => *geometry = written_geometry(attribute, written)?,
        FamilyValue::Relationship { objects, .. } => {
            let WrittenValue::Object(written_objects) = written else {
                return Err(missing());
            };
            *objects = written_objects.clone(); // clone: the stored instance owns its copy of the borrowed fragment.
        }
        FamilyValue::LanguageProperty { entries, .. } => {
            let WrittenValue::LanguageMap(written_entries) = written else {
                return Err(missing());
            };
            match depth {
                ValueDepth::Merge => merge_language_entries(entries, written_entries),
                ValueDepth::Replace => *entries = settled_entries(written_entries),
            }
        }
        FamilyValue::VocabProperty { vocab, .. } => {
            let WrittenValue::Vocab(written_vocab) = written else { return Err(missing()) };
            *vocab = written_vocab.clone(); // clone: the stored instance owns its copy of the borrowed fragment.
        }
        FamilyValue::JsonProperty { json, .. } => {
            let WrittenValue::Json(written_json) = written else { return Err(missing()) };
            write_json(json, written_json, depth).map_err(|_| missing())?;
        }
        FamilyValue::ListProperty { items, .. } => {
            let WrittenValue::ValueList(written_items) = written else {
                return Err(missing());
            };
            *items = settled_values(written_items);
        }
        FamilyValue::ListRelationship { objects, .. } => {
            let WrittenValue::ObjectList(written_objects) = written else {
                return Err(missing());
            };
            objects.clone_from(written_objects); // clone: the stored instance owns its copy of the borrowed fragment.
        }
    }
    Ok(())
}

/// Write a `JsonProperty`'s literals: a single literal on both sides merges
/// by IETF RFC 7396 under [`ValueDepth::Merge`], anything else is replaced.
fn write_json(json: &mut NonEmpty<JsonLiteral>, written: &NonEmpty<JsonLiteral>, depth: ValueDepth) -> Result<(), EmptyCollection> {
    if let (ValueDepth::Merge, [held], [patch]) = (depth, json.as_mut_slice(), written.as_slice()) {
        merge_json(held, patch);
        return Ok(());
    }
    *json = NonEmpty::try_from_vec(written.iter().map(settled_json).collect())?;
    Ok(())
}

/// Apply the companion members a written instance names (`unitCode`,
/// `valueType`, `objectType`) to a family value: a set member is written, a
/// member set to the NGSI-LD Null is deleted, an omitted one is kept.
///
/// # Errors
///
/// Returns [`EntityRefusal::UnitCodeForbidden`] or
/// [`EntityRefusal::MemberForbidden`] when the instance sets a member its
/// family never defines.
pub(crate) fn apply_companion_members(attribute: &AttributeName, value: &mut FamilyValue, document: &InstanceDocument) -> Result<(), MutationError> {
    let family = value.family();
    if let Some(slot) = document.unit_code() {
        match value {
            FamilyValue::Property { unit_code, .. } | FamilyValue::GeoProperty { unit_code, .. } | FamilyValue::ListProperty { unit_code, .. } => {
                *unit_code = slot.as_set().copied();
            }
            FamilyValue::Relationship { .. }
            | FamilyValue::LanguageProperty { .. }
            | FamilyValue::VocabProperty { .. }
            | FamilyValue::JsonProperty { .. }
            | FamilyValue::ListRelationship { .. } => {
                if let Slot::Set(_) = slot {
                    return Err(EntityRefusal::UnitCodeForbidden {
                        family,
                        attribute: attribute.clone(), // clone: the refusal outlives the borrowed request.
                    }
                    .into());
                }
            }
        }
    }
    if let Some(slot) = document.value_type() {
        match value {
            FamilyValue::Property { value_type, .. }
            | FamilyValue::GeoProperty { value_type, .. }
            | FamilyValue::LanguageProperty { value_type, .. }
            | FamilyValue::VocabProperty { value_type, .. }
            | FamilyValue::JsonProperty { value_type, .. }
            | FamilyValue::ListProperty { value_type, .. } => {
                *value_type = slot.as_set().cloned(); // clone: the stored instance owns its copy of the borrowed fragment.
            }
            FamilyValue::Relationship { .. } | FamilyValue::ListRelationship { .. } => {
                if let Slot::Set(_) = slot {
                    return Err(MutationError::from(EntityRefusal::member_forbidden(
                        Some(attribute),
                        Some(family),
                        properties::VALUE_TYPE,
                    )));
                }
            }
        }
    }
    if let Some(slot) = document.object_types() {
        match value {
            FamilyValue::Relationship { object_types, .. } | FamilyValue::ListRelationship { object_types, .. } => {
                *object_types = slot.as_set().cloned().unwrap_or_default(); // clone: the stored instance owns its copy of the borrowed fragment.
            }
            FamilyValue::Property { .. }
            | FamilyValue::GeoProperty { .. }
            | FamilyValue::LanguageProperty { .. }
            | FamilyValue::VocabProperty { .. }
            | FamilyValue::JsonProperty { .. }
            | FamilyValue::ListProperty { .. } => {
                if let Slot::Set(_) = slot {
                    return Err(MutationError::from(EntityRefusal::member_forbidden(
                        Some(attribute),
                        Some(family),
                        properties::OBJECT_TYPE,
                    )));
                }
            }
        }
    }
    Ok(())
}

/// The geometry a written `GeoProperty` value carries.
///
/// A fragment that declares no family carries its value as plain expanded
/// values; written onto a `GeoProperty` they are read as its geometry.
fn written_geometry(attribute: &AttributeName, written: &WrittenValue) -> Result<GeoValue, MutationError> {
    match written {
        WrittenValue::Geometry(geometry) => Ok(geometry.clone()), // clone: the stored instance owns its copy of the borrowed fragment.
        WrittenValue::Value(values) => match geo_value_of(attribute, values.as_slice())? {
            GeoDecoded::Value(geometry) => Ok(geometry),
            GeoDecoded::Null => Err(EntityRefusal::ReservedNullValue {
                attribute: attribute.clone(), // clone: the refusal outlives the borrowed request.
            }
            .into()),
        },
        WrittenValue::Object(_)
        | WrittenValue::LanguageMap(_)
        | WrittenValue::Vocab(_)
        | WrittenValue::Json(_)
        | WrittenValue::ValueList(_)
        | WrittenValue::ObjectList(_) => Err(MutationError::from(EntityRefusal::missing_value(attribute, AttributeType::GeoProperty))),
    }
}

/// Written values settled into the non-empty values of a Property.
fn settled_non_empty(attribute: &AttributeName, family: AttributeType, values: &[ExpandedValue<Fragment>]) -> Result<NonEmpty<ExpandedValue>, MutationError> {
    NonEmpty::try_from_vec(settled_values(values)).map_err(|_| MutationError::from(EntityRefusal::missing_value(attribute, family)))
}

#[cfg(test)]
mod tests {
    use crate::{
        document::{
            instance_document::InstanceDocument,
            slot::Slot,
            written_value::{ValueSlot, WrittenValue},
        },
        family_value::FamilyValue,
        non_empty::NonEmpty,
        operation::{
            family_write::{ValueDepth, apply_companion_members, fresh_family_value, write_family_value},
            mutation_error::MutationError,
            speed_entity::{name, number},
        },
        refusal::EntityRefusal,
        unit_code::UnitCode,
    };
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;

    fn relationship() -> FamilyValue {
        FamilyValue::Relationship {
            objects: NonEmpty::one("urn:ngsi-ld:X:1".parse().expect("a valid URI")),
            object_types: Vec::new(),
        }
    }

    /// Clause 4.5.3 defines a Relationship without `unitCode`.
    #[test]
    fn a_unit_code_on_a_relationship_is_refused() {
        let mut value = relationship();
        let written = InstanceDocument::builder()
            .unit_code(Some(Slot::Set(UnitCode::resolve("KMH").expect("a registered code"))))
            .build();
        let refused = apply_companion_members(&name("https://example.org/r"), &mut value, &written).expect_err("a unitless family");
        assert!(
            matches!(refused, MutationError::Refused(ref refusal) if matches!(**refusal, EntityRefusal::UnitCodeForbidden { .. })),
            "{refused:?}"
        );
    }

    /// A deleted `unitCode` on a family without one deletes nothing.
    #[test]
    fn a_unit_code_deletion_on_a_relationship_deletes_nothing() {
        let mut value = relationship();
        let written = InstanceDocument::builder().unit_code(Some(Slot::Remove)).build();
        apply_companion_members(&name("https://example.org/r"), &mut value, &written).expect("nothing to delete");
        pretty_assertions::assert_eq!(value, relationship());
    }

    /// A value written under another family's member names no value of this
    /// family.
    #[test]
    fn a_value_under_another_familys_member_is_the_missing_value_refusal() {
        let written = WrittenValue::Value(NonEmpty::one(number(1)));
        let refused = fresh_family_value(&name("https://example.org/r"), AttributeType::Relationship, &written).expect_err("not an object");
        assert!(
            matches!(refused, MutationError::Refused(ref refusal) if matches!(**refusal, EntityRefusal::MissingObject { .. })),
            "{refused:?}"
        );
        let mut value = relationship();
        assert!(write_family_value(&name("https://example.org/r"), &mut value, &written, ValueDepth::Merge).is_err());
    }

    /// A written value replaces a held Property value under
    /// [`ValueDepth::Replace`].
    #[test]
    fn a_replaced_property_value_takes_the_written_one() {
        let mut value = FamilyValue::Property {
            values: NonEmpty::one(number(1)),
            unit_code: None,
            value_type: None,
        };
        let written = InstanceDocument::builder()
            .value(ValueSlot::Set(WrittenValue::Value(NonEmpty::one(number(2)))))
            .build();
        let ValueSlot::Set(written) = written.value() else {
            panic!("a set value");
        };
        write_family_value(&name("https://example.org/p"), &mut value, written, ValueDepth::Replace).expect("a Property value");
        pretty_assertions::assert_eq!(
            value,
            FamilyValue::Property {
                values: NonEmpty::one(number(2)),
                unit_code: None,
                value_type: None,
            }
        );
    }
}
