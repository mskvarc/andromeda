//! Partial Attribute Update (clause 5.6.4), with the partial-update patch
//! behaviour of clause 5.5.8.

use crate::{
    document::{instance_document::InstanceDocument, written_value::ValueSlot},
    entity_state::EntityState,
    operation::{
        attribute_write::{InstanceRemovalOutcome, remove_instance},
        instance_patch::{InstancePatchDepth, patch_instance},
        mutation_error::MutationError,
        selection::addressed_dataset,
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};

impl EntityState {
    /// Patch `fragment` into one instance of `attribute` (clause 5.6.4).
    ///
    /// The instance is the one clause 5.6.4.4 names: "as an instance with the
    /// specified datasetId if present", otherwise the default instance, as
    /// [`addressed_dataset`] resolves it. Each member the fragment names
    /// replaces the stored one wholesale, a member set to the NGSI-LD Null is
    /// deleted, and an omitted member survives (clause 5.5.8); a sub-attribute
    /// is replaced instance by instance, each written instance replacing only
    /// the one with its `datasetId` or added beside the others. A fragment that
    /// is its family's deletion encoding deletes the instance, which clause
    /// 5.5.8 hands to clause 5.6.5: the attribute goes with its last instance.
    ///
    /// # Errors
    ///
    /// Returns [`MutationError::AttributeNotFound`],
    /// [`MutationError::DatasetNotFound`] or
    /// [`MutationError::DefaultInstanceNotFound`] when the addressed instance
    /// does not exist, [`MutationError::AttributeTypeMismatch`] when the
    /// fragment names another family ("it is not allowed to change the type
    /// of an Attribute"), and [`MutationError::Refused`] when the patched
    /// instance cannot be state.
    pub fn partial_update_attribute(
        &mut self,
        attribute: &AttributeName,
        dataset_id: Option<&Identifier>,
        fragment: &InstanceDocument,
    ) -> Result<(), MutationError> {
        let Some(held) = self.attributes().get(attribute) else {
            return Err(MutationError::AttributeNotFound {
                attribute: attribute.clone(), // clone: the error outlives the borrowed request.
            });
        };
        let addressed = addressed_dataset(dataset_id, fragment);
        if !held.contains(addressed) {
            return Err(MutationError::missing_instance(attribute, addressed));
        }
        if let ValueSlot::Remove(_) = fragment.value() {
            return match remove_instance(self.attributes_mut(), attribute, addressed) {
                InstanceRemovalOutcome::Removed => Ok(()),
                InstanceRemovalOutcome::Absent => Err(MutationError::missing_instance(attribute, addressed)),
            };
        }
        let instance = self
            .attributes_mut()
            .get_mut(attribute)
            .and_then(|instances| instances.get_mut(addressed))
            .ok_or_else(|| MutationError::missing_instance(attribute, addressed))?;
        patch_instance(attribute, instance, fragment, InstancePatchDepth::PartialUpdate, None)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attributes::Attributes,
        document::{
            attribute_member::AttributeMember,
            instance_document::InstanceDocument,
            slot::Slot,
            value_carrier::ValueCarrier,
            written_value::{ValueSlot, WrittenValue},
        },
        expanded_value::{ExpandedValue, ValueNode},
        family_value::FamilyValue,
        instance::Instance,
        non_empty::NonEmpty,
        operation::{
            mutation_error::MutationError,
            speed_entity::{
                ACCURACY,
                SPEED,
                dataset,
                date_time,
                entity,
                instance_rows,
                instances,
                integer_of,
                name,
                number,
                property,
                property_document,
                row,
                text,
                values,
                with_three_instances,
            },
        },
        refusal::EntityRefusal,
        unit_code::UnitCode,
        value_mode::FragmentMember,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, identifier_length::IDENTIFIER_BYTE_LIMIT, uri::Uri};
    use indexmap::IndexMap;
    use serde_json::json;

    const WEATHER: &str = "https://example.org/hasWeatherObserved";

    /// One stored Property instance carrying every member the cases patch:
    /// a value, a `unitCode`, an `observedAt` and a Relationship
    /// sub-attribute.
    fn stored_property() -> Instance {
        let sensor: Uri = "urn:ngsi-ld:Sensor:1".parse().expect("a valid URI");
        let relationship = Instance::builder()
            .value(FamilyValue::Relationship {
                objects: NonEmpty::one(sensor),
                object_types: Vec::new(),
            })
            .build();
        let mut sub_attributes = Attributes::new();
        sub_attributes.insert(name(WEATHER), instances(vec![(None, relationship)]));
        Instance::builder()
            .value(FamilyValue::Property {
                values: vec![number(4)],
                unit_code: Some(UnitCode::resolve("C62").expect("a registered code")),
                value_type: None,
            })
            .observed_at(Some(date_time("2020-09-09T16:40:00Z")))
            .sub_attributes(sub_attributes)
            .build()
    }

    fn patched(fragment: &InstanceDocument) -> Instance {
        let mut target = entity(vec![(SPEED, instances(vec![(None, stored_property())]))]);
        target.partial_update_attribute(&name(SPEED), None, fragment).expect("the patch applies");
        target
            .attributes()
            .get(SPEED)
            .and_then(|held| held.get(None))
            .cloned()
            .expect("the patched instance")
    }

    /// Clause 5.5.8 EXAMPLE 1: a member the Fragment names is replaced and a
    /// member it omits is left untouched.
    #[test]
    fn a_member_the_fragment_names_is_replaced_and_the_others_survive() {
        let instance = patched(&property_document(number(100), None));
        pretty_assertions::assert_eq!(integer_of(&instance), Some(100));
        pretty_assertions::assert_eq!(instance.value().unit_code(), UnitCode::resolve("C62").ok());
        assert!(instance.sub_attributes().contains(WEATHER));
    }

    /// Clause 5.5.8: a member the target does not carry is added.
    #[test]
    fn a_member_absent_from_the_target_is_added() {
        let mut target = entity(vec![(SPEED, instances(vec![(None, property(number(4)))]))]);
        let unit = UnitCode::resolve("CEL").expect("a registered code");
        target
            .partial_update_attribute(&name(SPEED), None, &InstanceDocument::builder().unit_code(Some(Slot::Set(unit))).build())
            .expect("the patch applies");
        let instance = target.attributes().get(SPEED).and_then(|held| held.get(None)).expect("the instance");
        pretty_assertions::assert_eq!(instance.value().unit_code(), Some(unit));
    }

    /// Clause 5.5.8: a non-reified member set to the NGSI-LD Null is deleted,
    /// and nothing else is.
    #[test]
    fn a_non_reified_member_set_to_the_null_sentinel_is_deleted() {
        let unit_deleted = patched(&InstanceDocument::builder().unit_code(Some(Slot::Remove)).build());
        pretty_assertions::assert_eq!(unit_deleted.value().unit_code(), None);
        pretty_assertions::assert_eq!(integer_of(&unit_deleted), Some(4));
        assert!(unit_deleted.sub_attributes().contains(WEATHER));

        let observed_deleted = patched(&InstanceDocument::builder().observed_at(Some(Slot::Remove)).build());
        pretty_assertions::assert_eq!(observed_deleted.observed_at(), None);
        pretty_assertions::assert_eq!(integer_of(&observed_deleted), Some(4));
        assert!(observed_deleted.sub_attributes().contains(WEATHER));
    }

    /// Clause 4.5.3: an NGSI-LD Null on the right-hand side of a sub-attribute's
    /// `object` deletes that Relationship rather than leaving it valueless.
    #[test]
    fn a_sub_attribute_whose_value_is_the_null_sentinel_is_deleted_whole() {
        let mut sub_attributes = IndexMap::new();
        sub_attributes.insert(
            name(WEATHER),
            AttributeMember::Instances(vec![
                InstanceDocument::builder()
                    .family(Some(AttributeType::Relationship))
                    .value(ValueSlot::Remove(ValueCarrier::Object))
                    .build(),
            ]),
        );
        let instance = patched(&InstanceDocument::builder().sub_attributes(sub_attributes).build());
        assert!(!instance.sub_attributes().contains(WEATHER));
        pretty_assertions::assert_eq!(instance.value().unit_code(), UnitCode::resolve("C62").ok());
    }

    /// Clause 5.5.8: a sub-attribute member "is only replaced if the
    /// datasetId is the same, otherwise the member of the Fragment is added as
    /// a new instance to the target".
    #[test]
    fn a_partial_update_adds_a_sub_attribute_instance_with_another_dataset_id() {
        let mut held = property(number(10));
        held.sub_attributes_mut()
            .insert(name(ACCURACY), instances(vec![(Some("urn:ngsi-ld:Dataset:A"), property(number(1)))]));
        let mut target = entity(vec![(SPEED, instances(vec![(None, held)]))]);
        let mut fragment = property_document(number(20), None);
        fragment.sub_attributes_mut().insert(
            name(ACCURACY),
            AttributeMember::Instances(vec![property_document(number(2), Some("urn:ngsi-ld:Dataset:B"))]),
        );

        target.partial_update_attribute(&name(SPEED), None, &fragment).expect("the patch applies");

        let instance = target.attributes().get(SPEED).and_then(|held| held.get(None)).expect("the instance");
        pretty_assertions::assert_eq!(integer_of(instance), Some(20));
        pretty_assertions::assert_eq!(
            instance_rows(instance.sub_attributes(), ACCURACY),
            vec![row(Some("urn:ngsi-ld:Dataset:A"), 1), row(Some("urn:ngsi-ld:Dataset:B"), 2)]
        );
    }

    /// Clause 5.6.4.4: "it is not allowed to change the type of an
    /// Attribute"; a fragment declaring the stored family keeps it.
    #[test]
    fn the_fragment_family_never_changes_the_stored_one() {
        let instance = patched(&InstanceDocument::builder().family(Some(AttributeType::Property)).build());
        pretty_assertions::assert_eq!(instance.family(), AttributeType::Property);
    }

    /// Clause 5.5.8 replaces a member wholesale rather than merging into it, so
    /// a Property value object the Fragment names loses the keys it omits.
    #[test]
    fn a_value_object_is_replaced_rather_than_merged_into() {
        let node = |members: Vec<(&str, &str)>| {
            ExpandedValue::Node(ValueNode::new(
                None,
                Vec::new(),
                members.into_iter().map(|(member, value)| (name(member), vec![text(value)])).collect(),
            ))
        };
        let mut target = entity(vec![(
            SPEED,
            instances(vec![(
                None,
                property(node(vec![("https://example.org/street", "a"), ("https://example.org/city", "b")])),
            )]),
        )]);
        let written = ExpandedValue::Node(ValueNode::new(
            None,
            Vec::new(),
            [(name("https://example.org/street"), FragmentMember::Values(vec![text("c")]))]
                .into_iter()
                .collect(),
        ));

        target
            .partial_update_attribute(&name(SPEED), None, &property_document(written, None))
            .expect("the patch applies");

        let instance = target.attributes().get(SPEED).and_then(|held| held.get(None)).expect("the instance");
        pretty_assertions::assert_eq!(
            instance.value(),
            &FamilyValue::Property {
                values: vec![node(vec![("https://example.org/street", "c")])],
                unit_code: None,
                value_type: None,
            }
        );
    }

    #[test]
    fn a_type_mismatch_in_a_partial_update_is_a_typed_error() {
        let mut target = entity(vec![(SPEED, instances(vec![(None, property(number(10)))]))]);
        let fragment = InstanceDocument::builder()
            .family(Some(AttributeType::Relationship))
            .value(ValueSlot::Set(WrittenValue::Object(NonEmpty::one(
                "urn:ngsi-ld:Vehicle:2".parse().expect("a valid URI"),
            ))))
            .build();

        let error = target.partial_update_attribute(&name(SPEED), None, &fragment).expect_err("must fail");

        assert!(
            matches!(
                &error,
                MutationError::AttributeTypeMismatch { attribute, fragment_type: AttributeType::Relationship, instance_type: AttributeType::Property }
                    if attribute.as_str() == SPEED
            ),
            "{error:?}"
        );
        pretty_assertions::assert_eq!(
            error.to_string(),
            "attribute type mismatch for https://example.com/speed: fragment declares [Relationship], instance is [Property]"
        );
    }

    #[test]
    fn partially_updating_an_absent_attribute_is_a_typed_error() {
        let mut target = entity(vec![("https://example.com/other", instances(vec![(None, property(number(1)))]))]);
        let error = target
            .partial_update_attribute(&name(SPEED), None, &InstanceDocument::builder().build())
            .expect_err("must fail");
        assert!(
            matches!(&error, MutationError::AttributeNotFound { attribute } if attribute.as_str() == SPEED),
            "{error:?}"
        );
    }

    /// Clause 5.5.8 hands a fragment that is the NGSI-LD Null to clause 5.6.5:
    /// the addressed instance is deleted and the others stay.
    #[test]
    fn a_deletion_fragment_deletes_only_the_addressed_instance() {
        let mut target = with_three_instances();
        let deletion = InstanceDocument::builder().value(ValueSlot::Remove(ValueCarrier::Value)).build();

        target
            .partial_update_attribute(&name(SPEED), Some(&dataset("urn:ngsi-ld:Dataset:A")), &deletion)
            .expect("the deletion applies");

        pretty_assertions::assert_eq!(values(&target, SPEED), vec![20, 30]);
    }

    /// Clause 5.6.4.4: the URL `datasetId` addresses the instance; one the
    /// entity lacks is a `ResourceNotFound`.
    #[test]
    fn partially_updating_an_absent_dataset_is_a_typed_error() {
        let mut target = with_three_instances();
        let error = target
            .partial_update_attribute(&name(SPEED), Some(&dataset("urn:ngsi-ld:Dataset:Z")), &property_document(number(1), None))
            .expect_err("must fail");
        assert!(
            matches!(&error, MutationError::DatasetNotFound { dataset_id, .. } if dataset_id.as_str() == "urn:ngsi-ld:Dataset:Z"),
            "{error:?}"
        );
    }

    /// Clause 4.5.5: a fragment whose `datasetId` cannot be read names no
    /// instance the broker can identify, so it never reaches the write at
    /// all: decoding refuses it, whatever a URL `datasetId` addresses.
    #[test]
    fn a_partial_update_with_an_unreadable_dataset_id_never_touches_the_default_instance() {
        let long_dataset = format!("urn:ngsi-ld:Dataset:{}", "a".repeat(IDENTIFIER_BYTE_LIMIT));
        let fragment = json!({
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 99}],
            "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": long_dataset}]
        });
        let refused = InstanceDocument::from_slice(&name(SPEED), &serde_json::to_vec(&fragment).expect("JSON")).expect_err("an unreadable datasetId");
        assert!(matches!(refused, EntityRefusal::DatasetId { .. }), "{refused:?}");
        let not_a_uri = json!({"https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "not a uri"}]});
        assert!(InstanceDocument::from_slice(&name(SPEED), &serde_json::to_vec(&not_a_uri).expect("JSON")).is_err());
    }
}
