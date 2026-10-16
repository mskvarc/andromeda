//! The observation a write leaves for the Temporal Evolution: the attribute
//! instances the write addressed, as they stand once it has landed.
//!
//! Clauses 4.5.7 and 4.5.8 compose the Temporal Evolution of an attribute of
//! "the sequence of instances of the referred Property", "each one
//! representing an instance of the Property (as mandated by clause 4.5.2)". A
//! request fragment is not such an instance. Under clause 5.5.8 a partial
//! update "only changes the elements provided" (EXAMPLE 1 keeps the `unitCode`
//! the fragment left out), under clause 5.5.12 a merge reaches into the value
//! itself, and clause 5.6.19.4 keeps the replaced instance's `createdAt`. What
//! history records is therefore read off the state the write produced, for
//! exactly the instances the write addressed and applied. An instance the
//! write deleted is absent from that state; its end is recorded by a tombstone
//! instead.

use crate::{
    attribute_instances::AttributeInstances,
    attributes::Attributes,
    document::{EntityDocument, instance_document::InstanceDocument, written_value::ValueSlot},
    entity_state::EntityState,
    instance::Instance,
    operation::selection::OverwritePolicy,
    scopes::Scopes,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use std::sync::Arc;

/// The attribute instances one write addressed.
#[derive(Clone, Copy, Debug)]
pub enum AddressedInstances<'a> {
    /// Every instance an entity fragment names, under the overwrite policy the
    /// write ran with (clauses 5.6.2, 5.6.3, 5.6.8, 5.6.9, 5.6.17, 5.6.18 and
    /// 5.6.20).
    EntityFragment {
        /// The entity fragment the request carried.
        fragment: &'a EntityDocument,
        /// Whether instances the entity already held were overwritten.
        overwrite: OverwritePolicy,
    },
    /// The one instance an attribute fragment addressed through its own
    /// `datasetId` (clauses 5.6.4 and 5.6.19).
    AttributeFragment {
        /// Expanded name of the attribute the URL names.
        attribute: &'a AttributeName,
        /// The attribute fragment the request carried.
        fragment: &'a InstanceDocument,
    },
    /// Every instance of the entity the write stored, which replaced the
    /// entity whole (clauses 5.6.8 replace mode and 5.6.18): the replacement
    /// is its own fragment, so each of its instances was addressed and
    /// applied, together with its scope.
    Replacement,
}

/// The observation a write leaves for the Temporal Evolution, or `None` when
/// it applied no attribute instance and no scope.
///
/// `previous` is the entity before the write (`None` when the write created
/// it) and `current` the entity it stored. The observation carries the
/// identifier and types of `current` and, for every instance `addressed` names
/// that the write applied, that instance as `current` holds it.
///
/// An instance is left out when the write did not apply it: under
/// [`OverwritePolicy::Preserve`] an instance the entity already held is kept
/// as it was (clause 5.6.3.4), and an instance the write deleted, or never
/// stored because clause 5.6.2.4 has it ignored, is not in `current`. The
/// scope is observed as `current` holds it, and only when the write changed
/// it: a write restating the Scope the entity held sets no new validity
/// (clause 4.18), and [`restated_scope`] names that Scope instead. An
/// attribute observed exactly as `current` holds it is shared with `current`,
/// not copied.
#[must_use]
pub fn written_observation(addressed: AddressedInstances<'_>, previous: Option<&EntityState>, current: &EntityState) -> Option<EntityState> {
    let mut observed = Attributes::new();
    let mut scope = None;
    match addressed {
        AddressedInstances::EntityFragment { fragment, overwrite } => {
            if fragment.scope().is_some() {
                // clone: the observation owns its copy of the scope the entity holds.
                scope = changed_scope(scope_write(current, previous, overwrite)).cloned();
            }
            for (name, member) in fragment.attributes() {
                if let Some(instances) = written_instances(name, member.instances(), current, previous, overwrite) {
                    // clone: the observation owns the attribute name.
                    observed.insert_shared(name.clone(), instances);
                }
            }
        }
        AddressedInstances::AttributeFragment { attribute, fragment } => {
            // Clause 5.5.8 hands a fragment that is the NGSI-LD Null to clause
            // 5.6.5: the addressed instance is deleted, not observed.
            if !matches!(fragment.value(), ValueSlot::Remove(_)) {
                let dataset_id = fragment.dataset_id();
                // Clause 5.4: a Fragment naming no member is a JSON Merge Patch
                // that returns its target unchanged, so it applies no instance.
                if let Some(instance) = current
                    .attributes()
                    .get(attribute)
                    .and_then(|held| held.get(dataset_id))
                    .filter(|_| !fragment.names_nothing())
                {
                    // clone: the observation owns the attribute name, the instance identity and a copy of the instance the entity holds.
                    observed.insert(attribute.clone(), AttributeInstances::one(dataset_id.cloned(), instance.clone()));
                }
            }
        }
        AddressedInstances::Replacement => {
            // clone: the observation owns its copy of the scope the entity holds.
            scope = changed_scope(scope_write(current, previous, OverwritePolicy::Overwrite)).cloned();
            // clone: reference-count bumps; the observation shares every attribute the replacement holds.
            observed = current.attributes().clone();
        }
    }
    if observed.is_empty() && scope.is_none() {
        return None;
    }
    // clone: the observation owns its copy of the entity's identity.
    Some(EntityState::from_parts(current.id().clone(), current.types().clone(), scope, None, observed))
}

/// The resulting instances of one attribute the entity fragment named and the
/// write applied, in the fragment's order.
fn written_instances(
    name: &AttributeName,
    documents: &[InstanceDocument],
    current: &EntityState,
    previous: Option<&EntityState>,
    overwrite: OverwritePolicy,
) -> Option<Arc<AttributeInstances>> {
    let held = current.attributes().get_shared(name)?;
    let applied: Vec<(Option<&Identifier>, &Instance)> = documents
        .iter()
        .filter(|document| !matches!(document.value(), ValueSlot::Remove(_)))
        // Clause 5.6.3.4 leaves an instance the entity already held untouched
        // when overwriting is disabled, so the write observed nothing of it.
        .filter(|document| {
            overwrite == OverwritePolicy::Overwrite
                || !previous
                    .and_then(|previous| previous.attributes().get(name))
                    .is_some_and(|instances| instances.contains(document.dataset_id()))
        })
        .filter_map(|document| held.get(document.dataset_id()).map(|instance| (document.dataset_id(), instance)))
        .collect();
    let exactly_held = applied.len() == held.len()
        && applied
            .iter()
            .zip(held.iter())
            .all(|((dataset_id, _), (held_dataset_id, _))| *dataset_id == held_dataset_id);
    if exactly_held {
        // clone: reference-count bump; the observation shares the instances the entity holds.
        return Some(Arc::clone(held));
    }
    let mut applied = applied.into_iter();
    let (dataset_id, instance) = applied.next()?;
    // clone: the observation owns the identity and a copy of each instance it records.
    let mut observed = AttributeInstances::one(dataset_id.cloned(), instance.clone());
    for (dataset_id, instance) in applied {
        observed.insert(dataset_id.cloned(), instance.clone());
    }
    Some(Arc::new(observed))
}

/// The Scope a write named restated unchanged, when it overwrote or replaced
/// the entity's Scope with the one the entity already held.
///
/// Clause 4.18 makes a Scope valid "from the time it has been set", and a write
/// restating the Scope already in force sets nothing new, so the write records
/// no change of Scope. It still states that the entity is in that Scope, which
/// the Temporal Evolution may not hold: Delete Temporal Evolution (clause
/// 5.6.16), retention and Temporal API writes all edit history apart from
/// current state. `None` when the write named no Scope, changed it, left the
/// entity without one, or did not overwrite it (clause 5.6.3.4).
#[must_use]
pub fn restated_scope<'a>(addressed: AddressedInstances<'_>, previous: Option<&EntityState>, current: &'a EntityState) -> Option<&'a Scopes> {
    let write = match addressed {
        AddressedInstances::EntityFragment { fragment, overwrite } => {
            fragment.scope()?;
            scope_write(current, previous, overwrite)
        }
        AddressedInstances::AttributeFragment { .. } => return None,
        AddressedInstances::Replacement => scope_write(current, previous, OverwritePolicy::Overwrite),
    };
    match write {
        ScopeWrite::Restated(scope) => Some(scope),
        ScopeWrite::Changed(_) | ScopeWrite::Untouched => None,
    }
}

/// What a write that named the entity's Scope did to it.
#[derive(Clone, Copy, Debug)]
enum ScopeWrite<'a> {
    /// The write left the entity in a Scope it did not hold before.
    Changed(&'a Scopes),
    /// The write overwrote the Scope with the one the entity already held.
    Restated(&'a Scopes),
    /// The write left no Scope, or applied none.
    Untouched,
}

/// What the write left of the entity's Scope, `previous` being `None` when
/// the write created the entity.
fn scope_write<'a>(current: &'a EntityState, previous: Option<&EntityState>, overwrite: OverwritePolicy) -> ScopeWrite<'a> {
    let Some(scope) = current.scope() else {
        return ScopeWrite::Untouched;
    };
    if previous.and_then(EntityState::scope) != Some(scope) {
        return ScopeWrite::Changed(scope);
    }
    // Clause 5.6.3.4 appends only "the Scopes in the Fragment that are not part
    // of the value of scope of the target Entity" when overwriting is
    // disabled, so a scope that came out unchanged is one the write did not
    // apply.
    match overwrite {
        OverwritePolicy::Overwrite => ScopeWrite::Restated(scope),
        OverwritePolicy::Preserve => ScopeWrite::Untouched,
    }
}

/// The Scope a write changed, when it changed one.
const fn changed_scope(write: ScopeWrite<'_>) -> Option<&Scopes> {
    match write {
        ScopeWrite::Changed(scope) => Some(scope),
        ScopeWrite::Restated(_) | ScopeWrite::Untouched => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_instances::AttributeInstances,
        attributes::Attributes,
        document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument, slot::Slot},
        entity_state::EntityState,
        entity_types::EntityTypes,
        family_value::FamilyValue,
        history::written_observation::{AddressedInstances, restated_scope, written_observation},
        instance::Instance,
        non_empty::NonEmpty,
        operation::{
            selection::OverwritePolicy,
            speed_entity::{dataset, date_time, fragment, name, number, observed_document, property, property_deletion, property_document, text},
        },
        scopes::Scopes,
        system_instant::SystemInstant,
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use cefact_units::UnitCode;
    use indexmap::IndexMap;
    use serde_json::json;
    use std::sync::Arc;

    const TEMPERATURE: &str = "https://example.org/temperature";
    const ADDRESS: &str = "https://example.org/address";
    const NAME: &str = "https://example.org/name";

    /// Attributes by name, each with its instances by `datasetId`.
    type Members<'a> = Vec<(&'a str, Vec<(Option<&'a str>, Instance)>)>;

    /// The building entity holding the given attributes.
    fn building(attributes: Members<'_>) -> EntityState {
        let attributes = attributes
            .into_iter()
            .map(|(attribute, instances)| {
                let instances = AttributeInstances::try_from_instances(instances.into_iter().map(|(dataset_id, instance)| (dataset_id.map(dataset), instance)))
                    .expect("distinct datasetIds")
                    .expect("at least one instance");
                (name(attribute), instances)
            })
            .collect::<Attributes>();
        EntityState::from_parts(
            "urn:ngsi-ld:Building:1".parse().expect("a valid identifier"),
            EntityTypes::one("https://example.org/Building".parse().expect("a valid type")),
            None,
            None,
            attributes,
        )
    }

    /// A Property holding an integer, a `unitCode` and an `observedAt`.
    fn measured(value: i64, observed_at: &str) -> Instance {
        let mut instance = Instance::builder()
            .value(FamilyValue::Property {
                values: vec![number(value)],
                unit_code: Some(UnitCode::Cel),
                value_type: None,
            })
            .build();
        instance.set_observed_at(Some(date_time(observed_at)));
        instance
    }

    fn attribute_fragment<'a>(fragment: &'a InstanceDocument, attribute: &'a AttributeName) -> AddressedInstances<'a> {
        AddressedInstances::AttributeFragment { attribute, fragment }
    }

    fn entity_fragment(fragment: &EntityDocument, overwrite: OverwritePolicy) -> AddressedInstances<'_> {
        AddressedInstances::EntityFragment { fragment, overwrite }
    }

    fn scope(paths: &[&str]) -> Scopes {
        Scopes::try_from_paths(paths.iter().map(|path| path.parse().expect("a valid scope path"))).expect("at least one path")
    }

    /// Clause 5.5.8 EXAMPLE 1: the partial update leaves `unitCode` in place,
    /// so the observed instance is the patched one carrying it, not the
    /// fragment.
    #[test]
    fn a_partial_attribute_update_observes_the_patched_instance_with_its_untouched_unit_code() {
        let previous = building(vec![(TEMPERATURE, vec![(None, measured(25, "2022-03-14T01:59:26.535Z"))])]);
        let current = building(vec![(TEMPERATURE, vec![(None, measured(100, "2022-03-14T13:00:00.000Z"))])]);
        let written = observed_document(100, None, "2022-03-14T13:00:00.000Z");
        let attribute = name(TEMPERATURE);

        let observed = written_observation(attribute_fragment(&written, &attribute), Some(&previous), &current);

        pretty_assertions::assert_eq!(observed, Some(current));
    }

    /// Clause 5.4: an attribute fragment naming no member is a JSON Merge Patch
    /// that returns its target unchanged, so it applies no instance and
    /// nothing is observed; one naming a member still observes the instance.
    #[test]
    fn an_empty_attribute_fragment_observes_nothing() {
        let held = building(vec![(TEMPERATURE, vec![(None, measured(25, "2022-03-14T01:59:26.535Z"))])]);
        let attribute = name(TEMPERATURE);
        let empty = InstanceDocument::builder().build();
        assert!(empty.names_nothing());
        pretty_assertions::assert_eq!(written_observation(attribute_fragment(&empty, &attribute), Some(&held), &held), None);

        let writes_a_value = observed_document(25, None, "2022-03-14T01:59:26.535Z");
        assert!(!writes_a_value.names_nothing());
        assert!(written_observation(attribute_fragment(&writes_a_value, &attribute), Some(&held), &held).is_some());
    }

    /// Clause 5.6.19.4 keeps the replaced instance's `createdAt`, which only
    /// the stored state carries.
    #[test]
    fn a_replaced_attribute_is_observed_with_the_created_at_the_replacement_kept() {
        let created_at: SystemInstant = "2022-01-01T00:00:00.000000Z".parse().expect("a valid instant");
        let mut before = property(text("long"));
        before.set_created_at(Some(created_at));
        let mut after = property(text("short"));
        after.set_created_at(Some(created_at));
        let current = building(vec![(TEMPERATURE, vec![(None, after)])]);
        let written = property_document(text("short"), None);
        let attribute = name(TEMPERATURE);

        let observed = written_observation(
            attribute_fragment(&written, &attribute),
            Some(&building(vec![(TEMPERATURE, vec![(None, before)])])),
            &current,
        );

        pretty_assertions::assert_eq!(observed, Some(current));
    }

    /// Only the addressed instance of a multi-attribute is observed; the other
    /// instance did not change (clause 4.5.5).
    #[test]
    fn an_attribute_fragment_observes_only_the_instance_its_dataset_id_addresses() {
        let state = building(vec![(
            TEMPERATURE,
            vec![
                (Some("urn:ngsi-ld:Dataset:a"), property(number(1))),
                (Some("urn:ngsi-ld:Dataset:b"), property(number(2))),
            ],
        )]);
        let written = property_document(number(2), Some("urn:ngsi-ld:Dataset:b"));
        let attribute = name(TEMPERATURE);

        let observed = written_observation(attribute_fragment(&written, &attribute), Some(&state), &state);

        pretty_assertions::assert_eq!(
            observed,
            Some(building(vec![(TEMPERATURE, vec![(Some("urn:ngsi-ld:Dataset:b"), property(number(2)))])]))
        );
    }

    /// Clause 5.5.8 hands an NGSI-LD Null fragment to clause 5.6.5: a deletion
    /// is a tombstone, never an observation.
    #[test]
    fn an_attribute_fragment_that_deletes_the_instance_observes_nothing() {
        let written = property_deletion(None);
        let attribute = name(TEMPERATURE);

        let observed = written_observation(
            attribute_fragment(&written, &attribute),
            Some(&building(vec![(TEMPERATURE, vec![(None, property(number(1)))])])),
            &building(Vec::new()),
        );

        pretty_assertions::assert_eq!(observed, None);
    }

    /// Clause 5.5.12 EXAMPLE 2: the merge keeps `city`, which the fragment
    /// never restated, so the observed value is the merged one.
    #[test]
    fn a_merged_attribute_is_observed_with_the_value_members_the_fragment_left_out() {
        let json_property = |value: serde_json::Value| {
            Instance::builder()
                .value(FamilyValue::JsonProperty {
                    json: NonEmpty::one(JsonLiteral::admit(&value.to_string()).expect("a JSON literal")),
                    value_type: None,
                })
                .build()
        };
        let previous = building(vec![(
            ADDRESS,
            vec![(
                None,
                json_property(json!({"street": "Straße des 17. Juni", "city": "Berlin", "country": "Germany"})),
            )],
        )]);
        let current = building(vec![(
            ADDRESS,
            vec![(None, json_property(json!({"street": "Pariser Platz", "city": "Berlin"})))],
        )]);
        let written = fragment(vec![(ADDRESS, AttributeMember::Instances(vec![InstanceDocument::builder().build()]))]);

        let observed = written_observation(entity_fragment(&written, OverwritePolicy::Overwrite), Some(&previous), &current);

        pretty_assertions::assert_eq!(observed, Some(current));
    }

    /// Only the attributes the fragment named are observed; the entity's other
    /// attributes did not change.
    #[test]
    fn an_entity_fragment_observes_only_the_attributes_it_named() {
        let previous = building(vec![(TEMPERATURE, vec![(None, property(number(21)))])]);
        let current = building(vec![
            (NAME, vec![(None, property(text("lobby")))]),
            (TEMPERATURE, vec![(None, property(number(21)))]),
        ]);
        let written = fragment(vec![(NAME, AttributeMember::Instances(vec![property_document(text("lobby"), None)]))]);

        let observed = written_observation(entity_fragment(&written, OverwritePolicy::Overwrite), Some(&previous), &current);

        pretty_assertions::assert_eq!(observed, Some(building(vec![(NAME, vec![(None, property(text("lobby")))])])));
    }

    /// Clause 5.6.3.4: with overwriting disabled an instance the entity
    /// already held is not appended, so it is not observed either; a new one
    /// is.
    #[test]
    fn a_no_overwrite_append_observes_only_the_instances_it_appended() {
        let previous = building(vec![
            (TEMPERATURE, vec![(None, property(number(21)))]),
            (NAME, vec![(None, property(text("hall")))]),
        ]);
        let current = building(vec![
            (
                TEMPERATURE,
                vec![(None, property(number(21))), (Some("urn:ngsi-ld:Dataset:b"), property(number(22)))],
            ),
            (NAME, vec![(None, property(text("hall")))]),
        ]);
        let written = fragment(vec![
            (
                TEMPERATURE,
                AttributeMember::Instances(vec![
                    property_document(number(99), None),
                    property_document(number(22), Some("urn:ngsi-ld:Dataset:b")),
                ]),
            ),
            (NAME, AttributeMember::Instances(vec![property_document(text("lobby"), None)])),
        ]);

        let observed = written_observation(entity_fragment(&written, OverwritePolicy::Preserve), Some(&previous), &current);

        pretty_assertions::assert_eq!(
            observed,
            Some(building(vec![(TEMPERATURE, vec![(Some("urn:ngsi-ld:Dataset:b"), property(number(22)))])]))
        );
    }

    /// A no-overwrite append that appended nothing observes nothing.
    #[test]
    fn a_no_overwrite_append_that_appended_nothing_observes_nothing() {
        let state = building(vec![(TEMPERATURE, vec![(None, property(number(21)))])]);
        let written = fragment(vec![(TEMPERATURE, AttributeMember::Instances(vec![property_document(number(99), None)]))]);

        pretty_assertions::assert_eq!(
            written_observation(entity_fragment(&written, OverwritePolicy::Preserve), Some(&state), &state),
            None
        );
    }

    /// An instance the fragment deletes is absent from the stored state and is
    /// recorded by a tombstone, not observed.
    #[test]
    fn an_entity_fragment_does_not_observe_the_instance_it_deleted() {
        let previous = building(vec![
            (TEMPERATURE, vec![(None, property(number(1)))]),
            (NAME, vec![(None, property(text("hall")))]),
        ]);
        let current = building(vec![(NAME, vec![(None, property(text("lobby")))])]);
        let written = fragment(vec![
            (TEMPERATURE, AttributeMember::Instances(vec![property_deletion(None)])),
            (NAME, AttributeMember::Instances(vec![property_document(text("lobby"), None)])),
        ]);

        let observed = written_observation(entity_fragment(&written, OverwritePolicy::Overwrite), Some(&previous), &current);

        pretty_assertions::assert_eq!(observed, Some(current));
    }

    /// Clause 5.6.2.4 ignores a `scope` the target entity does not hold, so the
    /// write observed no scope.
    #[test]
    fn a_scope_the_update_ignored_is_not_observed() {
        let state = building(Vec::new());
        let written = EntityDocument::new(None, Vec::new(), Some(Slot::Set(scope(&["/Madrid"]))), None, IndexMap::new());

        pretty_assertions::assert_eq!(
            written_observation(entity_fragment(&written, OverwritePolicy::Overwrite), Some(&state), &state),
            None
        );
    }

    /// Clause 5.6.3.4 appends scopes without overwriting: the observation is
    /// the scope the entity ends up in, and nothing when the append added
    /// none.
    #[test]
    fn a_no_overwrite_scope_append_is_observed_as_the_resulting_scope_only_when_it_changed() {
        let written = EntityDocument::new(None, Vec::new(), Some(Slot::Set(scope(&["/Madrid"]))), None, IndexMap::new());
        let mut before = building(Vec::new());
        before.set_scope(Some(scope(&["/Berlin"])));
        let mut after = building(Vec::new());
        after.set_scope(Some(scope(&["/Berlin", "/Madrid"])));

        let observed = written_observation(entity_fragment(&written, OverwritePolicy::Preserve), Some(&before), &after);
        pretty_assertions::assert_eq!(observed.as_ref().and_then(EntityState::scope), Some(&scope(&["/Berlin", "/Madrid"])));

        pretty_assertions::assert_eq!(
            written_observation(entity_fragment(&written, OverwritePolicy::Preserve), Some(&after), &after),
            None
        );
    }

    /// The entity's own members are not attributes and are never observed as
    /// one.
    #[test]
    fn reserved_entity_members_are_not_observed_as_attributes() {
        let mut state = building(Vec::new());
        state.set_expires_at(Some(date_time("2030-01-01T00:00:00Z")));
        let written = EntityDocument::new(None, Vec::new(), None, Some(Slot::Set(date_time("2030-01-01T00:00:00Z"))), IndexMap::new());

        pretty_assertions::assert_eq!(
            written_observation(entity_fragment(&written, OverwritePolicy::Overwrite), Some(&state), &state),
            None
        );
    }

    /// Clauses 5.6.8 (replace mode) and 5.6.18: a replacement addressed every
    /// instance it holds, so each is observed as the stored entity holds it,
    /// together with its scope, and shared rather than copied. The entity's
    /// own `expiresAt` is not an attribute and is not observed.
    #[test]
    fn a_replacement_observes_every_instance_and_the_scope_it_holds() {
        let mut current = building(vec![
            (
                TEMPERATURE,
                vec![(None, property(number(2))), (Some("urn:ngsi-ld:Dataset:b"), property(number(3)))],
            ),
            (NAME, vec![(None, property(text("lobby")))]),
        ]);
        current.set_scope(Some(scope(&["/Madrid"])));
        current.set_expires_at(Some(date_time("2030-01-01T00:00:00Z")));
        let previous = building(vec![(ADDRESS, vec![(None, property(text("gone")))])]);

        let observed = written_observation(AddressedInstances::Replacement, Some(&previous), &current).expect("an observation");

        let mut expected = current.clone();
        expected.set_expires_at(None);
        pretty_assertions::assert_eq!(observed, expected);
        let shared = |state: &EntityState| Arc::clone(state.attributes().get_shared(TEMPERATURE).expect("the attribute"));
        assert!(Arc::ptr_eq(&shared(&current), &shared(&observed)));
    }

    /// A replacement holding neither an attribute nor a scope observes nothing.
    #[test]
    fn an_empty_replacement_observes_nothing() {
        let current = building(Vec::new());

        pretty_assertions::assert_eq!(written_observation(AddressedInstances::Replacement, None, &current), None);
    }

    /// An attribute observed exactly as the entity holds it is shared with the
    /// entity rather than copied.
    #[test]
    fn an_attribute_observed_whole_is_shared_with_the_entity() {
        let current = building(vec![(TEMPERATURE, vec![(None, property(number(21)))])]);
        let written = fragment(vec![(TEMPERATURE, AttributeMember::Instances(vec![property_document(number(21), None)]))]);

        let observed = written_observation(entity_fragment(&written, OverwritePolicy::Overwrite), None, &current).expect("an observation");

        let shared = |state: &EntityState| Arc::clone(state.attributes().get_shared(TEMPERATURE).expect("the attribute"));
        assert!(Arc::ptr_eq(&shared(&current), &shared(&observed)));
    }

    /// Clause 4.18: a merge restating the Scope the entity held sets nothing
    /// new, so the observation carries no Scope and the Scope is named as
    /// restated instead.
    #[test]
    fn an_overwriting_merge_restating_the_held_scope_observes_no_scope_and_restates_it() {
        let written = EntityDocument::new(None, Vec::new(), Some(Slot::Set(scope(&["/Madrid"]))), None, IndexMap::new());
        let mut held = building(Vec::new());
        held.set_scope(Some(scope(&["/Madrid"])));
        let addressed = entity_fragment(&written, OverwritePolicy::Overwrite);

        pretty_assertions::assert_eq!(written_observation(addressed, Some(&held), &held), None);
        pretty_assertions::assert_eq!(restated_scope(addressed, Some(&held), &held), Some(&scope(&["/Madrid"])));
    }

    /// A merge changing the Scope observes the new one and restates nothing.
    #[test]
    fn an_overwriting_merge_changing_the_scope_observes_it_and_restates_nothing() {
        let written = EntityDocument::new(None, Vec::new(), Some(Slot::Set(scope(&["/Madrid"]))), None, IndexMap::new());
        let mut before = building(Vec::new());
        before.set_scope(Some(scope(&["/Berlin"])));
        let mut after = building(Vec::new());
        after.set_scope(Some(scope(&["/Madrid"])));
        let addressed = entity_fragment(&written, OverwritePolicy::Overwrite);

        let observed = written_observation(addressed, Some(&before), &after);
        pretty_assertions::assert_eq!(observed.as_ref().and_then(EntityState::scope), Some(&scope(&["/Madrid"])));
        pretty_assertions::assert_eq!(restated_scope(addressed, Some(&before), &after), None);
    }

    /// Clause 5.6.3.4: a no-overwrite append adding no path applied nothing,
    /// so it neither observes nor restates the Scope.
    #[test]
    fn a_no_overwrite_append_adding_no_scope_path_restates_nothing() {
        let written = EntityDocument::new(None, Vec::new(), Some(Slot::Set(scope(&["/Madrid"]))), None, IndexMap::new());
        let mut held = building(Vec::new());
        held.set_scope(Some(scope(&["/Madrid"])));

        pretty_assertions::assert_eq!(restated_scope(entity_fragment(&written, OverwritePolicy::Preserve), Some(&held), &held), None);
    }

    /// A replacement holding the Scope the entity held restates it and
    /// observes only its attributes; one changing it observes the new Scope.
    #[test]
    fn a_replacement_restates_an_unchanged_scope_and_observes_a_changed_one() {
        let mut current = building(vec![(NAME, vec![(None, property(text("lobby")))])]);
        current.set_scope(Some(scope(&["/Madrid"])));
        let mut same = building(Vec::new());
        same.set_scope(Some(scope(&["/Madrid"])));
        let mut other = building(Vec::new());
        other.set_scope(Some(scope(&["/Berlin"])));

        let restated = written_observation(AddressedInstances::Replacement, Some(&same), &current).expect("an observation");
        pretty_assertions::assert_eq!(restated.scope(), None);
        pretty_assertions::assert_eq!(
            restated_scope(AddressedInstances::Replacement, Some(&same), &current),
            Some(&scope(&["/Madrid"]))
        );

        let changed = written_observation(AddressedInstances::Replacement, Some(&other), &current).expect("an observation");
        pretty_assertions::assert_eq!(changed.scope(), Some(&scope(&["/Madrid"])));
        pretty_assertions::assert_eq!(restated_scope(AddressedInstances::Replacement, Some(&other), &current), None);
    }

    /// An attribute fragment names no Scope.
    #[test]
    fn an_attribute_fragment_restates_no_scope() {
        let mut held = building(vec![(TEMPERATURE, vec![(None, property(number(1)))])]);
        held.set_scope(Some(scope(&["/Madrid"])));
        let written = property_document(number(2), None);
        let attribute = name(TEMPERATURE);

        pretty_assertions::assert_eq!(restated_scope(attribute_fragment(&written, &attribute), Some(&held), &held), None);
    }
}
