//! Merge Entity (clause 5.6.17), with the merge-patch behaviour of clause
//! 5.5.12.

use crate::{
    document::{EntityDocument, attribute_member::AttributeMember},
    entity_state::EntityState,
    operation::{attribute_write::apply_instances, instance_patch::InstancePatchDepth, mutation_error::MutationError, scope_write::replace_scope},
};
use andromeda_ngsi_ld_lexicon::time::written_date_time::DateTimeText;
use chrono::{DateTime, Utc};

impl EntityState {
    /// Merge `fragment` into the entity (clause 5.6.17, behaviour in clause
    /// 5.5.12).
    ///
    /// The fragment's types are added to the entity's (clause 5.5.12: "all
    /// included Entity Types are added, if they are not already contained").
    /// The `scope` is replaced whole or removed by the NGSI-LD Null (clause
    /// 5.6.17.4), never merged instance by instance, and so is the entity
    /// `expiresAt` (clause 4.22 gives an entity one). An attribute set to the
    /// NGSI-LD Null is deleted with every instance; an instance that is its
    /// family's deletion encoding deletes the instance with its `datasetId`;
    /// every other instance merges into the instance with its `datasetId` to
    /// arbitrary depth, or is added.
    ///
    /// `default_observed_at` is Table 6.5.3.4-1's `observedAt`: an instance
    /// that names no `observedAt` while the instance it merges into carries
    /// one takes it, spelled with the sub-second precision it carries.
    ///
    /// # Errors
    ///
    /// Returns [`MutationError::Refused`] when a written instance cannot
    /// become state.
    pub fn merge(&mut self, fragment: &EntityDocument, default_observed_at: Option<DateTime<Utc>>) -> Result<(), MutationError> {
        // clone: the entity owns the types it gains.
        self.types_mut().extend(fragment.types().iter().cloned());
        if let Some(scope) = fragment.scope() {
            replace_scope(self, scope);
        }
        if let Some(expires_at) = fragment.expires_at() {
            // clone: the entity owns its copy of the borrowed fragment's expiry.
            self.set_expires_at(expires_at.as_set().cloned());
        }
        let default_observed_at = default_observed_at.map(DateTimeText::from_instant);
        for (name, member) in fragment.attributes() {
            match member {
                AttributeMember::Remove => {
                    self.attributes_mut().remove(name);
                }
                AttributeMember::Instances(documents) => {
                    apply_instances(
                        self.attributes_mut(),
                        name,
                        documents,
                        InstancePatchDepth::DeepMerge,
                        default_observed_at.as_ref(),
                    )?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        document::{
            EntityDocument,
            attribute_member::AttributeMember,
            instance_document::InstanceDocument,
            slot::Slot,
            written_value::{ValueSlot, WrittenValue},
        },
        entity_state::EntityState,
        entity_types::EntityTypes,
        expanded_value::{ExpandedValue, ValueNode},
        family_value::FamilyValue,
        geo_value::{EncodedGeometry, GeoValue},
        instance::Instance,
        language_entries::{LanguageEntries, LanguageEntry},
        list_object::ListObject,
        non_empty::NonEmpty,
        operation::{
            mutation_error::MutationError,
            speed_entity::{
                ACCURACY,
                SPEED,
                dataset,
                dataset_ids,
                date_time,
                entity,
                fragment,
                instance_rows,
                instances,
                integer_of,
                member_name,
                name,
                number,
                observed_document,
                property,
                property_deletion,
                property_document,
                row,
                text,
                values,
            },
        },
        refusal::EntityRefusal,
        scopes::Scopes,
        value_mode::{Fragment, FragmentMember, FragmentText},
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, scope::path::ScopePath, time::written_date_time::DateTimeText};
    use andromeda_ngsi_ld_vocabulary::expanded::properties;
    use cefact_units::UnitCode;
    use chrono::{DateTime, Utc};
    use indexmap::IndexMap;
    use langtag::LangTagBuf;
    use serde_json::json;

    const ADDRESS: &str = "https://example.com/address";
    const STREET: &str = "https://example.com/street";
    const CITY: &str = "https://example.com/city";
    const COUNTRY: &str = "https://example.com/country";

    fn at(raw: &str) -> DateTime<Utc> {
        raw.parse().expect("a valid timestamp")
    }

    fn scope(paths: &[&str]) -> Scopes {
        Scopes::try_from_paths(paths.iter().map(|path| path.parse::<ScopePath>().expect("a valid scope path"))).expect("at least one path")
    }

    fn scope_fragment(scope: Slot<Scopes>) -> EntityDocument {
        EntityDocument::new(None, Vec::new(), Some(scope), None, IndexMap::new())
    }

    fn speed(documents: Vec<InstanceDocument>) -> EntityDocument {
        fragment(vec![(SPEED, AttributeMember::Instances(documents))])
    }

    /// A stored address Property whose value is a structured node.
    fn address(members: &[(&str, &str)]) -> Instance {
        let members = members.iter().map(|(member, value)| (member_name(member), vec![text(value)])).collect();
        property(ExpandedValue::Node(ValueNode::new(None, Vec::new(), members)))
    }

    /// The written structured value of an address Property.
    fn written_address(members: Vec<(&str, FragmentMember)>) -> InstanceDocument {
        let members = members.into_iter().map(|(member, value)| (member_name(member), value)).collect();
        property_document(ExpandedValue::Node(ValueNode::new(None, Vec::new(), members)), None)
    }

    /// The members of the stored address value, as `(name, text)` pairs.
    fn address_members(entity: &EntityState) -> Vec<(String, ExpandedValue)> {
        let instance = entity.attributes().get(ADDRESS).and_then(|held| held.get(None)).expect("the address instance");
        let FamilyValue::Property { values, .. } = instance.value() else {
            panic!("a Property");
        };
        let [ExpandedValue::Node(node)] = values.as_slice() else {
            panic!("a structured value");
        };
        node.members().iter().map(|(member, values)| (member.to_string(), values[0].clone())).collect()
    }

    #[test]
    fn a_mixed_merge_array_deletes_one_dataset_and_applies_another() {
        // Clause 5.6.17: a merge array that carries a deletion for datasetId A
        // alongside a live update for datasetId B must delete A and apply B.
        let mut target = entity(vec![(
            SPEED,
            instances(vec![
                (Some("urn:ngsi-ld:Dataset:A"), property(number(10))),
                (Some("urn:ngsi-ld:Dataset:B"), property(number(15))),
            ]),
        )]);
        let patch = speed(vec![
            property_deletion(Some("urn:ngsi-ld:Dataset:A")),
            property_document(number(20), Some("urn:ngsi-ld:Dataset:B")),
        ]);

        target.merge(&patch, None).expect("merge succeeds");

        pretty_assertions::assert_eq!(dataset_ids(&target, SPEED), vec![Some("urn:ngsi-ld:Dataset:B".to_owned())]);
        pretty_assertions::assert_eq!(values(&target, SPEED), vec![20]);
    }

    #[test]
    fn a_datasetless_null_sentinel_deletes_only_the_default_and_keeps_a_later_update() {
        let mut target = entity(vec![(
            SPEED,
            instances(vec![(None, property(number(10))), (Some("urn:ngsi-ld:Dataset:B"), property(number(15)))]),
        )]);
        let patch = speed(vec![property_deletion(None), property_document(number(20), Some("urn:ngsi-ld:Dataset:B"))]);

        target.merge(&patch, None).expect("merge succeeds");

        pretty_assertions::assert_eq!(dataset_ids(&target, SPEED), vec![Some("urn:ngsi-ld:Dataset:B".to_owned())]);
        pretty_assertions::assert_eq!(values(&target, SPEED), vec![20]);
    }

    #[test]
    fn a_bare_member_null_sentinel_deletes_every_attribute_instance() {
        let mut target = entity(vec![(
            SPEED,
            instances(vec![(None, property(number(10))), (Some("urn:ngsi-ld:Dataset:B"), property(number(15)))]),
        )]);

        target.merge(&fragment(vec![(SPEED, AttributeMember::Remove)]), None).expect("merge succeeds");

        assert!(!target.attributes().contains(SPEED));
    }

    #[test]
    fn a_datasetless_null_on_a_lone_default_instance_deletes_the_whole_attribute() {
        let mut target = entity(vec![(SPEED, instances(vec![(None, property(number(10)))]))]);

        target.merge(&speed(vec![property_deletion(None)]), None).expect("merge succeeds");

        assert!(!target.attributes().contains(SPEED), "the attribute is gone");
    }

    #[test]
    fn merge_recurses_into_a_value_object() {
        // Clause 5.5.12: Merge Entity descends into a Property value object, so
        // touching `street` leaves `city` standing.
        let mut target = entity(vec![(ADDRESS, instances(vec![(None, address(&[(STREET, "old"), (CITY, "town")]))]))]);
        let patch = fragment(vec![(
            ADDRESS,
            AttributeMember::Instances(vec![written_address(vec![(STREET, FragmentMember::Values(vec![text("new")]))])]),
        )]);

        target.merge(&patch, None).expect("merge succeeds");

        pretty_assertions::assert_eq!(
            address_members(&target),
            vec![(STREET.to_owned(), text("new")), (CITY.to_owned(), text("town"))]
        );
    }

    #[test]
    fn deep_merge_recurses_into_a_property_value_object() {
        // Clause 5.5.12 EXAMPLE 2: a merge that touches one member of a Property
        // value object keeps the untouched members and deletes a member set to
        // the NGSI-LD Null.
        let mut target = entity(vec![(
            ADDRESS,
            instances(vec![(None, address(&[(STREET, "old"), (CITY, "town"), (COUNTRY, "land")]))]),
        )]);
        let patch = fragment(vec![(
            ADDRESS,
            AttributeMember::Instances(vec![written_address(vec![
                (STREET, FragmentMember::Values(vec![text("new")])),
                (COUNTRY, FragmentMember::Remove),
            ])]),
        )]);

        target.merge(&patch, None).expect("merge succeeds");

        pretty_assertions::assert_eq!(
            address_members(&target),
            vec![(STREET.to_owned(), text("new")), (CITY.to_owned(), text("town"))]
        );
    }

    /// A merge that adds a structured value has nothing to delete the
    /// NGSI-LD Null member from, so the member is left out rather than stored
    /// (clauses 4.5.0, 5.5.12).
    #[test]
    fn a_deletion_inside_a_new_value_object_has_nothing_to_delete() {
        let mut target = entity(Vec::new());
        let patch = fragment(vec![(
            ADDRESS,
            AttributeMember::Instances(vec![written_address(vec![
                (STREET, FragmentMember::Values(vec![text("new")])),
                (COUNTRY, FragmentMember::Remove),
            ])]),
        )]);

        target.merge(&patch, None).expect("merge succeeds");

        pretty_assertions::assert_eq!(address_members(&target), vec![(STREET.to_owned(), text("new"))]);
    }

    #[test]
    fn a_default_observed_at_stamps_only_instances_that_previously_carried_one() {
        // Table 6.5.3.4-1 `observedAt`: the parameter fills in a missing
        // observedAt only where the target instance already had one, and never
        // overrides one the fragment names.
        let mut observed = property(number(10));
        observed.set_observed_at(Some(date_time("2020-01-01T00:00:00Z")));
        let mut target = entity(vec![
            (SPEED, instances(vec![(None, observed), (Some("urn:ngsi-ld:Dataset:B"), property(number(15)))])),
            ("https://example.com/brand", instances(vec![(None, property(text("old")))])),
        ]);
        let patch = fragment(vec![
            (
                SPEED,
                AttributeMember::Instances(vec![
                    property_document(number(11), None),
                    observed_document(20, Some("urn:ngsi-ld:Dataset:B"), "2021-06-01T00:00:00Z"),
                ]),
            ),
            (
                "https://example.com/brand",
                AttributeMember::Instances(vec![property_document(text("new"), None)]),
            ),
        ]);

        target.merge(&patch, Some(at("2024-02-03T04:05:06Z"))).expect("merge succeeds");

        let observed_at = |attribute: &str, dataset_id: Option<&str>| {
            let dataset_id = dataset_id.map(dataset);
            target
                .attributes()
                .get(attribute)
                .and_then(|held| held.get(dataset_id.as_ref()))
                .and_then(Instance::observed_at)
                .map(|observed| observed.as_str().to_owned())
        };
        pretty_assertions::assert_eq!(
            observed_at(SPEED, None).as_deref(),
            Some("2024-02-03T04:05:06Z"),
            "the default instance previously observed takes the parameter"
        );
        pretty_assertions::assert_eq!(
            observed_at(SPEED, Some("urn:ngsi-ld:Dataset:B")).as_deref(),
            Some("2021-06-01T00:00:00Z"),
            "an explicit observedAt in the fragment is kept"
        );
        pretty_assertions::assert_eq!(
            observed_at("https://example.com/brand", None),
            None,
            "an attribute that never carried observedAt gets none"
        );
    }

    /// Table 6.5.3.4-1 `observedAt` is stamped by the broker from the request
    /// parameter, so it keeps the milliseconds rather than truncating them to
    /// whole seconds (clause 4.6.3).
    #[test]
    fn a_default_observed_at_keeps_its_sub_second_precision() {
        let mut observed = property(number(10));
        observed.set_observed_at(Some(date_time("2020-01-01T00:00:00Z")));
        let mut target = entity(vec![(SPEED, instances(vec![(None, observed)]))]);

        target
            .merge(&speed(vec![property_document(number(11), None)]), Some(at("2026-03-01T10:00:00.123Z")))
            .expect("merge succeeds");

        let instance = target.attributes().get(SPEED).and_then(|held| held.get(None)).expect("the instance");
        pretty_assertions::assert_eq!(instance.observed_at().map(DateTimeText::as_str), Some("2026-03-01T10:00:00.123Z"));
    }

    /// Clause 4.18 makes scope non-reified, so it cannot carry the
    /// `observedAt` Table 6.5.3.4-1 stamps on merged attribute instances.
    #[test]
    fn a_default_observed_at_never_stamps_the_scope() {
        let mut observed = property(number(10));
        observed.set_observed_at(Some(date_time("2020-01-01T00:00:00Z")));
        let mut target = entity(vec![(SPEED, instances(vec![(None, observed)]))]);
        target.set_scope(Some(scope(&["/X/A", "/Y/A"])));
        let patch = EntityDocument::new(
            None,
            Vec::new(),
            Some(Slot::Set(scope(&["/A/B"]))),
            None,
            [(name(SPEED), AttributeMember::Instances(vec![property_document(number(11), None)]))]
                .into_iter()
                .collect(),
        );

        target.merge(&patch, Some(at("2024-02-03T04:05:06Z"))).expect("merge succeeds");

        pretty_assertions::assert_eq!(target.scope(), Some(&scope(&["/A/B"])));
        let instance = target.attributes().get(SPEED).and_then(|held| held.get(None)).expect("the instance");
        pretty_assertions::assert_eq!(instance.observed_at().map(DateTimeText::as_str), Some("2024-02-03T04:05:06Z"));
    }

    #[test]
    fn merge_entity_adds_new_attr() {
        let mut target = entity(vec![("https://example.com/a", instances(vec![(None, property(number(1)))]))]);

        target
            .merge(
                &fragment(vec![(
                    "https://example.com/b",
                    AttributeMember::Instances(vec![property_document(number(2), None)]),
                )]),
                None,
            )
            .expect("merge succeeds");

        pretty_assertions::assert_eq!(values(&target, "https://example.com/b"), vec![2]);
        pretty_assertions::assert_eq!(values(&target, "https://example.com/a"), vec![1]);
    }

    /// Clause 5.5.12 removes a target member the fragment sets to an NGSI-LD
    /// Null, and clause 4.18 makes that sentinel the deleted scope.
    #[test]
    fn merge_deletes_the_scope_the_null_sentinel_names() {
        let mut target = entity(Vec::new());
        target.set_scope(Some(scope(&["/Madrid"])));

        target.merge(&scope_fragment(Slot::Remove), None).expect("merge succeeds");

        pretty_assertions::assert_eq!(target.scope(), None);
    }

    /// Clause 5.6.17.4: the scope of the target "will become the one included
    /// in the Fragment".
    #[test]
    fn merge_replaces_a_scope_array_with_the_fragment_array() {
        let mut target = entity(Vec::new());
        target.set_scope(Some(scope(&["/X/A", "/Y/A", "/Z/A"])));
        target
            .merge(&scope_fragment(Slot::Set(scope(&["/A/B", "/C/D"]))), None)
            .expect("merge succeeds");
        pretty_assertions::assert_eq!(target.scope(), Some(&scope(&["/A/B", "/C/D"])));
    }

    #[test]
    fn merge_replaces_a_scope_array_with_a_single_scope() {
        let mut target = entity(Vec::new());
        target.set_scope(Some(scope(&["/X/A", "/Y/A", "/Z/A"])));
        target.merge(&scope_fragment(Slot::Set(scope(&["/G/H"]))), None).expect("merge succeeds");
        pretty_assertions::assert_eq!(target.scope(), Some(&scope(&["/G/H"])));
    }

    #[test]
    fn merge_replaces_a_single_scope_with_an_array() {
        let mut target = entity(Vec::new());
        target.set_scope(Some(scope(&["/M/N"])));
        target
            .merge(&scope_fragment(Slot::Set(scope(&["/A/B", "/C/D"]))), None)
            .expect("merge succeeds");
        pretty_assertions::assert_eq!(target.scope(), Some(&scope(&["/A/B", "/C/D"])));
    }

    /// Clause 5.5.12: members of the Fragment that do not appear within the
    /// target "are added to the target".
    #[test]
    fn merge_adds_a_scope_to_an_entity_without_one() {
        let mut target = entity(Vec::new());
        target
            .merge(&scope_fragment(Slot::Set(scope(&["/A/B", "/C/D"]))), None)
            .expect("merge succeeds");
        pretty_assertions::assert_eq!(target.scope(), Some(&scope(&["/A/B", "/C/D"])));
    }

    #[test]
    fn merge_with_an_overlapping_scope_takes_the_fragment_exactly() {
        let mut target = entity(Vec::new());
        target.set_scope(Some(scope(&["/X/A", "/Y/A"])));
        target
            .merge(&scope_fragment(Slot::Set(scope(&["/X/A", "/Q/R"]))), None)
            .expect("merge succeeds");
        pretty_assertions::assert_eq!(target.scope(), Some(&scope(&["/X/A", "/Q/R"])));
    }

    /// Clause 5.5.12 removes a member set to the NGSI-LD Null only when the
    /// target contains it, so a target without scope is left as it was.
    #[test]
    fn merge_of_a_scope_deletion_onto_an_entity_without_scope_changes_nothing() {
        let original = entity(Vec::new());
        let mut target = original.clone();
        target.merge(&scope_fragment(Slot::Remove), None).expect("merge succeeds");
        pretty_assertions::assert_eq!(target, original);
    }

    #[test]
    fn merge_leaves_the_scope_untouched_when_the_fragment_omits_it() {
        let mut target = entity(vec![(SPEED, instances(vec![(None, property(number(10)))]))]);
        target.set_scope(Some(scope(&["/X/A", "/Y/A"])));

        target.merge(&speed(vec![property_document(number(20), None)]), None).expect("merge succeeds");

        pretty_assertions::assert_eq!(target.scope(), Some(&scope(&["/X/A", "/Y/A"])));
        pretty_assertions::assert_eq!(values(&target, SPEED), vec![20]);
    }

    /// Clause 5.5.12: "all included Entity Types are added, if they are not
    /// already contained in the type member of the target".
    #[test]
    fn merge_adds_entity_types_the_target_lacks_without_duplicating_one() {
        let type_name = |raw: &str| raw.parse().expect("a valid type");
        let mut target = entity(Vec::new());
        *target.types_mut() = EntityTypes::one(type_name("https://example.com/A"));
        let patch = EntityDocument::new(
            None,
            vec![type_name("https://example.com/A"), type_name("https://example.com/B")],
            None,
            None,
            IndexMap::new(),
        );

        target.merge(&patch, None).expect("merge succeeds");

        pretty_assertions::assert_eq!(
            target.types().as_slice(),
            &[type_name("https://example.com/A"), type_name("https://example.com/B")]
        );
    }

    /// Clause 4.22 gives an entity one `expiresAt`, so merging a new one
    /// replaces the stored value.
    #[test]
    fn merge_replaces_the_entity_expiry() {
        let mut target = entity(Vec::new());
        target.set_expires_at(Some(date_time("2030-01-01T00:00:00Z")));
        let patch = EntityDocument::new(None, Vec::new(), None, Some(Slot::Set(date_time("2031-01-01T00:00:00Z"))), IndexMap::new());

        target.merge(&patch, None).expect("merge succeeds");

        pretty_assertions::assert_eq!(target.expires_at(), Some(&date_time("2031-01-01T00:00:00Z")));
    }

    #[test]
    fn deep_merging_an_absent_attribute_inserts_the_patch_instances() {
        let mut target = entity(Vec::new());
        target.merge(&speed(vec![property_document(number(10), None)]), None).expect("merge succeeds");
        pretty_assertions::assert_eq!(target.attributes().get(SPEED), Some(&instances(vec![(None, property(number(10)))])));
    }

    #[test]
    fn deep_merging_a_matching_dataset_id_merges_the_instance_members() {
        let mut target = entity(vec![(SPEED, instances(vec![(Some("urn:ngsi-ld:Dataset:A"), property(number(10)))]))]);
        let unit = UnitCode::Kmh;
        let patch = InstanceDocument::builder()
            .dataset_id(Some(dataset("urn:ngsi-ld:Dataset:A")))
            .value(ValueSlot::Set(WrittenValue::Value(vec![number(20)])))
            .unit_code(Some(Slot::Set(unit)))
            .build();

        target.merge(&speed(vec![patch]), None).expect("merge succeeds");

        let held = target.attributes().get(SPEED).expect("the attribute");
        pretty_assertions::assert_eq!(held.len(), 1);
        let instance = held.get(Some(&dataset("urn:ngsi-ld:Dataset:A"))).expect("the instance");
        pretty_assertions::assert_eq!(integer_of(instance), Some(20));
        pretty_assertions::assert_eq!(instance.value().unit_code(), Some(unit));
    }

    #[test]
    fn deep_merging_a_changed_attribute_type_replaces_the_whole_instance() {
        let mut existing = property(number(10));
        if let FamilyValue::Property { unit_code, .. } = existing.value_mut() {
            *unit_code = Some(UnitCode::Kmh);
        }
        let mut target = entity(vec![(SPEED, instances(vec![(None, existing)]))]);
        let vehicle = "urn:ngsi-ld:Vehicle:2".parse().expect("a valid URI");
        let patch = InstanceDocument::builder()
            .family(Some(AttributeType::Relationship))
            .value(ValueSlot::Set(WrittenValue::Object(NonEmpty::one(vehicle))))
            .build();

        target.merge(&speed(vec![patch]), None).expect("merge succeeds");

        let instance = target.attributes().get(SPEED).and_then(|held| held.get(None)).expect("the instance");
        pretty_assertions::assert_eq!(
            instance.value(),
            &FamilyValue::Relationship {
                objects: NonEmpty::one("urn:ngsi-ld:Vehicle:2".parse().expect("a valid URI")),
                object_types: Vec::new(),
            }
        );
    }

    #[test]
    fn deep_merging_an_unmatched_dataset_id_appends_a_new_instance() {
        let mut target = entity(vec![(SPEED, instances(vec![(Some("urn:ngsi-ld:Dataset:A"), property(number(10)))]))]);
        target
            .merge(&speed(vec![property_document(number(20), Some("urn:ngsi-ld:Dataset:B"))]), None)
            .expect("merge succeeds");
        pretty_assertions::assert_eq!(values(&target, SPEED), vec![10, 20]);
    }

    /// Clause 5.5.12 merges "up to an arbitrary depth", so a sub-attribute of
    /// a merged instance lands by `datasetId` as the attribute does (clause
    /// 5.6.17.4): the written instance B is added beside the held A.
    #[test]
    fn merge_lands_a_sub_attribute_by_dataset_id_inside_a_merged_attribute() {
        let mut held = property(number(10));
        held.sub_attributes_mut()
            .insert(name(ACCURACY), instances(vec![(Some("urn:ngsi-ld:Dataset:A"), property(number(1)))]));
        let mut target = entity(vec![(SPEED, instances(vec![(None, held)]))]);
        let mut written = property_document(number(20), None);
        written.sub_attributes_mut().insert(
            name(ACCURACY),
            AttributeMember::Instances(vec![property_document(number(2), Some("urn:ngsi-ld:Dataset:B"))]),
        );

        target.merge(&speed(vec![written]), None).expect("merge succeeds");

        pretty_assertions::assert_eq!(values(&target, SPEED), vec![20]);
        let instance = target.attributes().get(SPEED).and_then(|held| held.get(None)).expect("the instance");
        pretty_assertions::assert_eq!(
            instance_rows(instance.sub_attributes(), ACCURACY),
            vec![row(Some("urn:ngsi-ld:Dataset:A"), 1), row(Some("urn:ngsi-ld:Dataset:B"), 2)]
        );
    }

    /// Clause 5.5.12: the merge never touches the instance identity, so the
    /// merged instance stays the one its `datasetId` names.
    #[test]
    fn deep_merge_keeps_the_dataset_id_of_the_merged_instance() {
        let mut target = entity(vec![(SPEED, instances(vec![(Some("urn:ngsi-ld:Dataset:A"), property(number(10)))]))]);
        target
            .merge(&speed(vec![property_document(number(20), Some("urn:ngsi-ld:Dataset:A"))]), None)
            .expect("merge succeeds");
        pretty_assertions::assert_eq!(dataset_ids(&target, SPEED), vec![Some("urn:ngsi-ld:Dataset:A".to_owned())]);
        pretty_assertions::assert_eq!(values(&target, SPEED), vec![20]);
    }

    #[test]
    fn deep_merge_of_a_language_tag_keeps_the_other_tags() {
        // A LanguageProperty merge naming one tag replaces that tag and leaves
        // the rest of the language map standing (clause 4.5.18).
        let tag = |raw: &str| Some(LangTagBuf::new(raw.to_owned()).expect("a language tag"));
        let stored = Instance::builder()
            .value(FamilyValue::LanguageProperty {
                entries: LanguageEntries::new(vec![LanguageEntry::new(tag("en"), "hi".into()), LanguageEntry::new(tag("de"), "hallo".into())]),
                value_type: None,
            })
            .build();
        let mut target = entity(vec![(SPEED, instances(vec![(None, stored)]))]);
        let patch = InstanceDocument::builder()
            .family(Some(AttributeType::LanguageProperty))
            .value(ValueSlot::Set(WrittenValue::LanguageMap(LanguageEntries::<Fragment>::new(vec![
                LanguageEntry::new(tag("de"), FragmentText::Text("guten tag".into())),
            ]))))
            .build();

        target.merge(&speed(vec![patch]), None).expect("merge succeeds");

        let instance = target.attributes().get(SPEED).and_then(|held| held.get(None)).expect("the instance");
        pretty_assertions::assert_eq!(
            instance.value(),
            &FamilyValue::LanguageProperty {
                entries: LanguageEntries::new(vec![
                    LanguageEntry::new(tag("en"), "hi".into()),
                    LanguageEntry::new(tag("de"), "guten tag".into())
                ]),
                value_type: None,
            }
        );
    }

    /// A language set to the NGSI-LD Null loses its entry (clause 5.5.12).
    #[test]
    fn deep_merge_of_a_language_deletion_removes_only_that_language() {
        let tag = |raw: &str| Some(LangTagBuf::new(raw.to_owned()).expect("a language tag"));
        let stored = Instance::builder()
            .value(FamilyValue::LanguageProperty {
                entries: LanguageEntries::new(vec![LanguageEntry::new(tag("en"), "hi".into()), LanguageEntry::new(tag("de"), "hallo".into())]),
                value_type: None,
            })
            .build();
        let mut target = entity(vec![(SPEED, instances(vec![(None, stored)]))]);
        let patch = InstanceDocument::builder()
            .value(ValueSlot::Set(WrittenValue::LanguageMap(LanguageEntries::<Fragment>::new(vec![
                LanguageEntry::new(tag("de"), FragmentText::Remove),
            ]))))
            .build();

        target.merge(&speed(vec![patch]), None).expect("merge succeeds");

        let instance = target.attributes().get(SPEED).and_then(|held| held.get(None)).expect("the instance");
        pretty_assertions::assert_eq!(
            instance.value(),
            &FamilyValue::LanguageProperty {
                entries: LanguageEntries::new(vec![LanguageEntry::new(tag("en"), "hi".into())]),
                value_type: None,
            }
        );
    }

    /// Clause 5.5.12 reaches into a `JsonProperty`'s JSON object: a key set
    /// to the NGSI-LD Null string is deleted, a key the fragment omits
    /// survives.
    #[test]
    fn deep_merge_of_a_json_property_merges_its_object() {
        let stored = Instance::builder()
            .value(FamilyValue::JsonProperty {
                json: NonEmpty::one(
                    JsonLiteral::admit(&json!({"street": "Straße des 17. Juni", "city": "Berlin", "country": "Germany"}).to_string()).expect("a JSON literal"),
                ),
                value_type: None,
            })
            .build();
        let mut target = entity(vec![(ADDRESS, instances(vec![(None, stored)]))]);
        let patch = InstanceDocument::builder()
            .family(Some(AttributeType::JsonProperty))
            .value(ValueSlot::Set(WrittenValue::Json(NonEmpty::one(
                JsonLiteral::admit(&json!({"street": "Pariser Platz", "country": "urn:ngsi-ld:null"}).to_string()).expect("a JSON literal"),
            ))))
            .build();

        target
            .merge(&fragment(vec![(ADDRESS, AttributeMember::Instances(vec![patch]))]), None)
            .expect("merge succeeds");

        let instance = target.attributes().get(ADDRESS).and_then(|held| held.get(None)).expect("the instance");
        pretty_assertions::assert_eq!(
            instance.value(),
            &FamilyValue::JsonProperty {
                json: NonEmpty::one(JsonLiteral::admit(&json!({"street": "Pariser Platz", "city": "Berlin"}).to_string()).expect("a JSON literal")),
                value_type: None,
            }
        );
    }

    /// A typeless fragment writing `value` onto a `GeoProperty` is read as its
    /// geometry rather than turning it into a Property (clause 4.7).
    #[test]
    fn a_typeless_value_merged_onto_a_geo_property_is_read_as_its_geometry() {
        let point = |longitude: i64, latitude: i64| -> ExpandedValue<Fragment> {
            let mut members = IndexMap::new();
            members.insert(
                member_name("https://purl.org/geojson/vocab#coordinates"),
                FragmentMember::Values(vec![ExpandedValue::List(vec![number(longitude), number(latitude)])]),
            );
            ExpandedValue::Node(ValueNode::new(
                None,
                vec!["https://purl.org/geojson/vocab#Point".parse().expect("a valid type")],
                members,
            ))
        };
        let location = properties::LOCATION.as_str();
        let mut target = entity(Vec::new());
        target
            .merge(
                &fragment(vec![(
                    location,
                    AttributeMember::Instances(vec![
                        InstanceDocument::builder()
                            .family(Some(AttributeType::GeoProperty))
                            .value(ValueSlot::Set(WrittenValue::Geometry(GeoValue::Encoded(
                                EncodedGeometry::parse(r#"{"type":"Point","coordinates":[1,2]}"#.to_owned()).expect("a point"),
                            ))))
                            .build(),
                    ]),
                )]),
                None,
            )
            .expect("merge succeeds");

        target
            .merge(
                &fragment(vec![(
                    location,
                    AttributeMember::Instances(vec![property_document_without_family(point(3, 4))]),
                )]),
                None,
            )
            .expect("merge succeeds");

        let instance = target.attributes().get(location).and_then(|held| held.get(None)).expect("the instance");
        pretty_assertions::assert_eq!(instance.family(), AttributeType::GeoProperty);
    }

    /// A written `value` instance that declares no family.
    fn property_document_without_family(value: ExpandedValue<Fragment>) -> InstanceDocument {
        InstanceDocument::builder().value(ValueSlot::Set(WrittenValue::Value(vec![value]))).build()
    }

    /// A merged `ListRelationship` takes the written list whole.
    #[test]
    fn a_merged_object_list_replaces_the_stored_one() {
        let uri = |raw: &str| raw.parse().expect("a valid URI");
        let stored = Instance::builder()
            .value(FamilyValue::ListRelationship {
                objects: vec![ListObject::Bare(uri("urn:ngsi-ld:A:1")), ListObject::Bare(uri("urn:ngsi-ld:A:2"))],
                object_types: Vec::new(),
            })
            .build();
        let mut target = entity(vec![(SPEED, instances(vec![(None, stored)]))]);
        let patch = InstanceDocument::builder()
            .value(ValueSlot::Set(WrittenValue::ObjectList(vec![ListObject::Wrapped(uri("urn:ngsi-ld:A:3"))])))
            .build();

        target.merge(&speed(vec![patch]), None).expect("merge succeeds");

        let instance = target.attributes().get(SPEED).and_then(|held| held.get(None)).expect("the instance");
        pretty_assertions::assert_eq!(
            instance.value(),
            &FamilyValue::ListRelationship {
                objects: vec![ListObject::Wrapped(uri("urn:ngsi-ld:A:3"))],
                object_types: Vec::new(),
            }
        );
    }

    /// A fragment that cannot become state is refused with the family's
    /// missing-value refusal (clause 4.5.2).
    #[test]
    fn a_new_attribute_without_a_value_is_refused() {
        let mut target = entity(Vec::new());
        let patch = InstanceDocument::builder()
            .observed_at(Some(Slot::Set(date_time("2020-01-01T00:00:00Z"))))
            .build();
        let refused = target.merge(&speed(vec![patch]), None).expect_err("a valueless new attribute");
        assert!(
            matches!(refused, MutationError::Refused(ref refusal) if matches!(**refusal, EntityRefusal::MissingValue { .. })),
            "{refused:?}"
        );
    }
}
