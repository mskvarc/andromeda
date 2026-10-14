//! Update Attributes (clause 5.6.2), with the partial-update patch behaviour
//! of clause 5.5.8.

use crate::{
    document::{EntityDocument, attribute_member::AttributeMember, slot::Slot},
    entity_state::EntityState,
    operation::{attribute_write::apply_instances, instance_patch::InstancePatchDepth, mutation_error::MutationError, scope_write::replace_scope},
    reads::member_name::MemberName,
};

impl EntityState {
    /// Update the entity's attributes with `fragment` (clause 5.6.2).
    ///
    /// The fragment's types are added to the entity's. Each written instance
    /// replaces the whole instance with its `datasetId` (clause 5.5.8 EXAMPLE
    /// 2: a member the fragment omits is dropped), or is added; an instance
    /// that is its family's deletion encoding deletes the instance with its
    /// `datasetId`, and an attribute set to the NGSI-LD Null is deleted whole
    /// (clause 5.5.8: "For each member of the Fragment, whose value is an
    /// NGSI-LD Null, contained by the target, the target member is deleted").
    /// A deletion of something the entity does not hold deletes nothing and
    /// stores nothing.
    ///
    /// Clause 5.6.2.4 replaces the scope "if the target entity includes
    /// scope", and ignores it otherwise.
    ///
    /// Returns the members the update did not apply, which decides between
    /// 204 and 207.
    ///
    /// # Errors
    ///
    /// Returns [`MutationError::Refused`] when a written instance cannot
    /// become state.
    pub fn update_attributes<'a>(&mut self, fragment: &'a EntityDocument) -> Result<Vec<MemberName<'a>>, MutationError> {
        // clone: the entity owns the types it gains.
        self.types_mut().extend(fragment.types().iter().cloned());
        let mut not_updated = Vec::new();
        if let Some(scope) = fragment.scope() {
            if self.scope().is_some() {
                replace_scope(self, scope);
            } else {
                not_updated.push(MemberName::Scope);
            }
        }
        match fragment.expires_at() {
            // clone: the entity owns its copy of the borrowed fragment's expiry.
            Some(Slot::Set(expires_at)) => self.set_expires_at(Some(expires_at.clone())),
            Some(Slot::Remove) => self.set_expires_at(None),
            None => {}
        }
        for (name, member) in fragment.attributes() {
            match member {
                AttributeMember::Remove => {
                    self.attributes_mut().remove(name);
                }
                AttributeMember::Instances(documents) => {
                    apply_instances(self.attributes_mut(), name, documents, InstancePatchDepth::ReplaceInstance, None)?;
                }
            }
        }
        Ok(not_updated)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        document::{EntityDocument, attribute_member::AttributeMember, slot::Slot},
        entity_state::EntityState,
        family_value::FamilyValue,
        geo_value::GeoValue,
        geometry_value::GeometryValue,
        instance::Instance,
        operation::speed_entity::{SPEED, dataset_ids, entity, fragment, instances, name, number, property, property_deletion, property_document, values},
        reads::member_name::MemberName,
        scopes::Scopes,
    };
    use andromeda_ngsi_ld_geometry::{form::GeometryForm, position::Position};
    use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
    use andromeda_ngsi_ld_vocabulary::expanded::properties;
    use cefact_units::UnitCode;
    use indexmap::IndexMap;
    use std::sync::Arc;

    fn scope(paths: &[&str]) -> Scopes {
        Scopes::try_from_paths(paths.iter().map(|path| path.parse::<ScopePath>().expect("a valid scope path"))).expect("at least one path")
    }

    fn scope_fragment(scope: Slot<Scopes>) -> EntityDocument {
        EntityDocument::new(None, Vec::new(), Some(scope), None, IndexMap::new())
    }

    #[test]
    fn update_attributes_deletes_the_default_instance_and_keeps_the_named_update() {
        // Clause 5.6.2 updates each supplied instance; the default-instance
        // deletion cannot discard a named update later in the same member.
        let mut target = entity(vec![(
            SPEED,
            instances(vec![(None, property(number(10))), (Some("urn:ngsi-ld:Dataset:B"), property(number(15)))]),
        )]);
        let patch = fragment(vec![(
            SPEED,
            AttributeMember::Instances(vec![property_deletion(None), property_document(number(20), Some("urn:ngsi-ld:Dataset:B"))]),
        )]);

        let not_updated = target.update_attributes(&patch).expect("update succeeds");

        assert!(not_updated.is_empty(), "{not_updated:?}");
        pretty_assertions::assert_eq!(dataset_ids(&target, SPEED), vec![Some("urn:ngsi-ld:Dataset:B".to_owned())]);
        pretty_assertions::assert_eq!(values(&target, SPEED), vec![20]);
    }

    #[test]
    fn update_attributes_replaces_the_whole_instance_dropping_unmentioned_subattrs() {
        // Clause 5.5.8 EXAMPLE 2: Update Attributes overwrites the whole
        // matched instance, so a `unitCode` the patch omits is removed rather
        // than carried forward.
        let existing = Instance::builder()
            .value(FamilyValue::Property {
                values: vec![number(25)],
                unit_code: Some(UnitCode::Cel),
                value_type: None,
            })
            .build();
        let mut target = entity(vec![(SPEED, instances(vec![(None, existing)]))]);

        let written = fragment(vec![(SPEED, AttributeMember::Instances(vec![property_document(number(100), None)]))]);
        let not_updated = target.update_attributes(&written).expect("update succeeds");

        assert!(not_updated.is_empty(), "{not_updated:?}");
        pretty_assertions::assert_eq!(target.attributes().get(SPEED), Some(&instances(vec![(None, property(number(100)))])));
    }

    /// Clause 5.5.8: "For each member of the Fragment, whose value is an
    /// NGSI-LD Null, contained by the target, the target member is deleted."
    /// Clause 4.18 spells a deleted scope with that same sentinel.
    #[test]
    fn update_attributes_deletes_the_scope_the_null_sentinel_names() {
        let mut target = entity(Vec::new());
        target.set_scope(Some(scope(&["/Madrid"])));

        let written = scope_fragment(Slot::Remove);
        let not_updated = target.update_attributes(&written).expect("update succeeds");

        assert!(not_updated.is_empty(), "{not_updated:?}");
        pretty_assertions::assert_eq!(target.scope(), None);
    }

    /// Clause 5.5.8 deletes "each member of the Fragment, whose value is an
    /// NGSI-LD Null, contained by the target": a deletion the target does not
    /// contain has nothing to delete, and must not be stored as data.
    #[test]
    fn update_attributes_ignores_a_deletion_of_an_attribute_the_target_lacks() {
        let mut target = entity(Vec::new());

        let written = fragment(vec![(SPEED, AttributeMember::Instances(vec![property_deletion(None)]))]);
        let not_updated = target.update_attributes(&written).expect("update succeeds");

        assert!(not_updated.is_empty(), "{not_updated:?}");
        assert!(!target.attributes().contains(SPEED));
    }

    /// The same holds for the member-level NGSI-LD Null a fragment deletes a
    /// whole member with.
    #[test]
    fn update_attributes_ignores_a_bare_deletion_of_an_attribute_the_target_lacks() {
        let mut target = entity(Vec::new());
        target
            .update_attributes(&fragment(vec![(SPEED, AttributeMember::Remove)]))
            .expect("update succeeds");
        assert!(!target.attributes().contains(SPEED));
    }

    /// Clause 5.5.8: a member-level NGSI-LD Null on an attribute the target
    /// holds deletes it whole.
    #[test]
    fn update_attributes_deletes_a_held_attribute_set_to_the_null() {
        let mut target = entity(vec![(SPEED, instances(vec![(None, property(number(1)))]))]);
        target
            .update_attributes(&fragment(vec![(SPEED, AttributeMember::Remove)]))
            .expect("update succeeds");
        assert!(!target.attributes().contains(SPEED));
    }

    /// Clause 5.6.2.4: "If scope is included in the Fragment and the target
    /// entity includes scope, replace the scope by the one included in the
    /// Fragment".
    #[test]
    fn update_attributes_replaces_a_scope_the_target_already_carries() {
        let mut target = entity(Vec::new());
        target.set_scope(Some(scope(&["/Madrid"])));
        target
            .update_attributes(&scope_fragment(Slot::Set(scope(&["/Barcelona"]))))
            .expect("update succeeds");
        pretty_assertions::assert_eq!(target.scope(), Some(&scope(&["/Barcelona"])));
    }

    /// Clause 5.6.2.4 closes with "otherwise ignore it", so a fragment deleting
    /// a scope the target never had leaves the entity untouched and reports the
    /// member as one this request did not update.
    #[test]
    fn update_attributes_reports_a_scope_deletion_the_target_cannot_take() {
        let mut target = entity(Vec::new());

        let written = scope_fragment(Slot::Remove);
        let not_updated = target.update_attributes(&written).expect("update succeeds");

        pretty_assertions::assert_eq!(not_updated, vec![MemberName::Scope]);
        pretty_assertions::assert_eq!(target.scope(), None);
    }

    /// The entity's attributes are shared: updating `speed` on a clone of an
    /// entity that also holds a geometry of a million positions copies
    /// `speed` alone, and the geometry stays the very allocation both states
    /// point at.
    #[test]
    fn an_update_of_one_attribute_does_not_copy_another() {
        let positions = (0_u32..1_000_000)
            .map(|index| Position::try_new(f64::from(index % 360) - 180.0, f64::from(index % 180) - 90.0, None))
            .collect::<Result<Vec<_>, _>>()
            .expect("valid positions");
        let location = Instance::builder()
            .value(FamilyValue::GeoProperty {
                geometry: GeoValue::Geometry(GeometryValue::new(GeometryForm::MultiPoint(positions))),
                unit_code: None,
                value_type: None,
            })
            .build();
        let previous = entity(vec![
            (properties::LOCATION.as_str(), instances(vec![(None, location)])),
            (SPEED, instances(vec![(None, property(number(10)))])),
        ]);
        let mut next = previous.clone();

        next.update_attributes(&fragment(vec![(SPEED, AttributeMember::Instances(vec![property_document(number(20), None)]))]))
            .expect("update succeeds");

        let shared = |state: &EntityState, attribute: &str| Arc::clone(state.attributes().get_shared(attribute).expect("the attribute"));
        assert!(
            Arc::ptr_eq(&shared(&previous, properties::LOCATION.as_str()), &shared(&next, properties::LOCATION.as_str())),
            "the untouched geometry is not copied"
        );
        assert!(
            !Arc::ptr_eq(&shared(&previous, SPEED), &shared(&next, SPEED)),
            "the updated attribute is the new state's own"
        );
        pretty_assertions::assert_eq!(values(&previous, SPEED), vec![10]);
        pretty_assertions::assert_eq!(values(&next, SPEED), vec![20]);
    }

    /// An attribute the target lacks is added (clause 5.6.2).
    #[test]
    fn update_attributes_adds_an_attribute_the_target_lacks() {
        let mut target = entity(Vec::new());
        target
            .update_attributes(&fragment(vec![(SPEED, AttributeMember::Instances(vec![property_document(number(7), None)]))]))
            .expect("update succeeds");
        pretty_assertions::assert_eq!(values(&target, SPEED), vec![7]);
        assert!(target.attributes().contains(name(SPEED).as_str()));
    }
}
