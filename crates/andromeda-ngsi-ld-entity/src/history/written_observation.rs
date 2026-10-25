//! The observation a Core write leaves for the Temporal Evolution: the
//! attribute instances the write modified, as they stand once it has landed.
//!
//! Clause 4.3.1 updates the Temporal Evolution "whenever the 'current state'
//! is modified via the Core API", and clause 5.8.6 says what a modification of
//! an attribute is: "any of the members (including children) in its
//! corresponding JSON-LD node is updated with a value different than the
//! existing one". An instance is therefore observed when it is new or its
//! content differs from its predecessor's, by [`Instance::same_content`], the
//! one test the notification change uses too. A restatement of what the
//! entity already holds modifies nothing and records nothing, whatever the
//! request named, so a producer repeating its last report adds no instance.
//!
//! Clauses 4.5.7 and 4.5.8 compose the Temporal Evolution of an attribute of
//! "the sequence of instances of the referred Property", "each one
//! representing an instance of the Property (as mandated by clause 4.5.2)". A
//! request fragment is not such an instance. Under clause 5.5.8 a partial
//! update "only changes the elements provided" (EXAMPLE 1 keeps the `unitCode`
//! the fragment left out), under clause 5.5.12 a merge reaches into the value
//! itself, and clause 5.6.19.4 keeps the replaced instance's `createdAt`. What
//! history records is therefore read off the state the write produced. An
//! instance the write deleted is absent from that state; its end is recorded
//! by a tombstone instead.

use crate::{attribute_instances::AttributeInstances, attributes::Attributes, entity_state::EntityState, instance::Instance};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use std::sync::Arc;

/// The observation a write leaves for the Temporal Evolution, or `None` when
/// it modified no attribute instance and no Scope.
///
/// `previous` is the entity before the write (`None` when the write created
/// it) and `current` the entity it stored. The observation carries the
/// identifier and types of `current` and every instance of `current` that has
/// no predecessor of the same content, as `current` holds it.
///
/// The Scope is observed as `current` holds it when it differs from the Scope
/// of `previous`. Clause 4.18 makes a Scope valid "from the time it has been
/// set", and a write restating the Scope in force sets nothing. An attribute
/// whose instances are all observed is shared with `current`, not copied.
#[must_use]
pub fn written_observation(previous: Option<&EntityState>, current: &EntityState) -> Option<EntityState> {
    let held = previous.map(EntityState::attributes);
    let mut observed = Attributes::new();
    for (name, instances) in current.attributes().iter_shared() {
        if let Some(modified) = modified_instances(instances, held.and_then(|held| held.get_shared(name))) {
            // clone: the observation owns the attribute name.
            observed.insert_shared(name.clone(), modified);
        }
    }
    let scope = current.scope().filter(|scope| previous.and_then(EntityState::scope) != Some(*scope)).cloned(); // clone: the observation owns its copy of the Scope the entity holds.
    if observed.is_empty() && scope.is_none() {
        return None;
    }
    // clone: the observation owns its copy of the entity's identity.
    Some(EntityState::from_parts(current.id().clone(), current.types().clone(), scope, None, observed))
}

/// The instances of one attribute the write modified, in stored order, given
/// the instances the attribute held before it (`None` when it held none).
fn modified_instances(instances: &Arc<AttributeInstances>, predecessors: Option<&Arc<AttributeInstances>>) -> Option<Arc<AttributeInstances>> {
    let Some(predecessors) = predecessors else {
        // clone: reference-count bump; a new attribute is observed whole and shared.
        return Some(Arc::clone(instances));
    };
    // The very instances of the previous state: the write left them as they were.
    if Arc::ptr_eq(instances, predecessors) {
        return None;
    }
    let modified: Vec<(Option<&Identifier>, &Instance)> = instances
        .iter()
        .filter(|(dataset_id, instance)| predecessors.get(*dataset_id).is_none_or(|predecessor| !instance.same_content(predecessor)))
        .collect();
    if modified.len() == instances.len() {
        // clone: reference-count bump; the observation shares the instances the entity holds.
        return Some(Arc::clone(instances));
    }
    let mut modified = modified.into_iter();
    let (dataset_id, instance) = modified.next()?;
    // clone: the observation owns the identity and a copy of each instance it records.
    let mut observed = AttributeInstances::one(dataset_id.cloned(), instance.clone());
    for (dataset_id, instance) in modified {
        observed.insert(dataset_id.cloned(), instance.clone());
    }
    Some(Arc::new(observed))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        attribute_instances::AttributeInstances,
        attributes::Attributes,
        entity_state::EntityState,
        entity_types::EntityTypes,
        family_value::FamilyValue,
        history::written_observation::written_observation,
        instance::Instance,
        non_empty::NonEmpty,
        operation::speed_entity::{dataset, date_time, name, number, property, text},
        scopes::Scopes,
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::time::system_instant::SystemInstant;
    use cefact_units::UnitCode;
    use serde_json::json;
    use std::sync::Arc;

    const TEMPERATURE: &str = "https://example.org/temperature";
    const HUMIDITY: &str = "https://example.org/humidity";
    const ADDRESS: &str = "https://example.org/address";
    const NAME: &str = "https://example.org/name";
    const ACCURACY: &str = "https://example.org/accuracy";

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

    /// `instance` stamped with the clause 4.8 system times at `at`.
    fn stamped(mut instance: Instance, at: &str) -> Instance {
        let at: SystemInstant = at.parse().expect("a valid instant");
        instance.set_created_at(Some(at));
        instance.set_modified_at(Some(at));
        instance
    }

    /// `instance` carrying an `accuracy` sub-Property holding `accuracy`.
    fn with_accuracy(mut instance: Instance, accuracy: i64) -> Instance {
        instance
            .sub_attributes_mut()
            .insert(name(ACCURACY), AttributeInstances::one(None, property(number(accuracy))));
        instance
    }

    fn scope(paths: &[&str]) -> Scopes {
        Scopes::try_from_paths(paths.iter().map(|path| path.parse().expect("a valid scope path"))).expect("at least one path")
    }

    /// The building holding nothing but `paths` as its Scope.
    fn scoped(paths: &[&str]) -> EntityState {
        let mut state = building(Vec::new());
        state.set_scope(Some(scope(paths)));
        state
    }

    /// Clause 4.3.1 with clause 5.8.6: a write restating the value and
    /// `observedAt` an instance holds modifies nothing and observes nothing.
    #[test]
    fn an_instance_restated_with_the_same_content_observes_nothing() {
        let previous = building(vec![(TEMPERATURE, vec![(None, measured(25, "2022-03-14T01:59:26.535Z"))])]);
        let current = building(vec![(TEMPERATURE, vec![(None, measured(25, "2022-03-14T01:59:26.535Z"))])]);

        pretty_assertions::assert_eq!(written_observation(Some(&previous), &current), None);
    }

    /// Clause 4.8: `createdAt` and `modifiedAt` are not content, so a restated
    /// instance stamped at another instant still observes nothing.
    #[test]
    fn system_times_alone_are_no_modification() {
        let previous = building(vec![(TEMPERATURE, vec![(None, stamped(property(number(21)), "2026-01-01T00:00:00.000000Z"))])]);
        let current = building(vec![(TEMPERATURE, vec![(None, stamped(property(number(21)), "2026-02-01T00:00:00.000000Z"))])]);

        pretty_assertions::assert_eq!(written_observation(Some(&previous), &current), None);
    }

    /// Clause 5.8.6: a different value is a modification.
    #[test]
    fn a_changed_value_is_observed() {
        let previous = building(vec![(TEMPERATURE, vec![(None, property(number(21)))])]);
        let current = building(vec![(TEMPERATURE, vec![(None, property(number(22)))])]);

        pretty_assertions::assert_eq!(written_observation(Some(&previous), &current), Some(current));
    }

    /// Clause 5.8.6 counts the children of the node: a changed sub-attribute
    /// modifies the instance holding it.
    #[test]
    fn a_changed_sub_attribute_is_observed() {
        let previous = building(vec![(TEMPERATURE, vec![(None, with_accuracy(property(number(21)), 1))])]);
        let current = building(vec![(TEMPERATURE, vec![(None, with_accuracy(property(number(21)), 2))])]);

        pretty_assertions::assert_eq!(written_observation(Some(&previous), &current), Some(current));
    }

    /// `observedAt` is a member of the node (clause 4.5.1), so the same value
    /// observed at another instant is a new observation.
    #[test]
    fn the_same_value_at_another_observed_at_is_observed() {
        let previous = building(vec![(TEMPERATURE, vec![(None, measured(25, "2022-03-14T01:59:26.535Z"))])]);
        let current = building(vec![(TEMPERATURE, vec![(None, measured(25, "2022-03-14T02:00:00.000Z"))])]);

        pretty_assertions::assert_eq!(written_observation(Some(&previous), &current), Some(current));
    }

    /// The upsert of `temperature` and `humidity` changing only `temperature`
    /// observes `temperature` alone.
    #[test]
    fn only_the_modified_attribute_of_a_restating_write_is_observed() {
        let previous = building(vec![
            (HUMIDITY, vec![(None, measured(50, "2026-10-19T10:00:00Z"))]),
            (TEMPERATURE, vec![(None, measured(20, "2026-10-19T10:00:00Z"))]),
        ]);
        let current = building(vec![
            (HUMIDITY, vec![(None, measured(50, "2026-10-19T10:00:00Z"))]),
            (TEMPERATURE, vec![(None, measured(21, "2026-10-19T10:00:00Z"))]),
        ]);

        pretty_assertions::assert_eq!(
            written_observation(Some(&previous), &current),
            Some(building(vec![(TEMPERATURE, vec![(None, measured(21, "2026-10-19T10:00:00Z"))])]))
        );
    }

    /// Clause 5.5.8 EXAMPLE 1: the partial update leaves `unitCode` in place,
    /// so the observed instance is the patched one carrying it, not the
    /// fragment.
    #[test]
    fn a_partial_attribute_update_observes_the_patched_instance_with_its_untouched_unit_code() {
        let previous = building(vec![(TEMPERATURE, vec![(None, measured(25, "2022-03-14T01:59:26.535Z"))])]);
        let current = building(vec![(TEMPERATURE, vec![(None, measured(100, "2022-03-14T13:00:00.000Z"))])]);

        pretty_assertions::assert_eq!(written_observation(Some(&previous), &current), Some(current));
    }

    /// Clause 5.6.19.4 keeps the replaced instance's `createdAt`, which only
    /// the stored state carries.
    #[test]
    fn a_replaced_attribute_is_observed_with_the_created_at_the_replacement_kept() {
        let before = stamped(property(text("long")), "2022-01-01T00:00:00.000000Z");
        let mut after = stamped(property(text("short")), "2022-01-01T00:00:00.000000Z");
        after.set_modified_at(Some("2022-02-01T00:00:00.000000Z".parse().expect("a valid instant")));
        let current = building(vec![(TEMPERATURE, vec![(None, after)])]);

        let observed = written_observation(Some(&building(vec![(TEMPERATURE, vec![(None, before)])])), &current);

        pretty_assertions::assert_eq!(observed, Some(current));
    }

    /// Clause 4.5.5: only the modified instance of a multi-attribute is
    /// observed.
    #[test]
    fn only_the_modified_instance_of_a_multi_attribute_is_observed() {
        let previous = building(vec![(
            TEMPERATURE,
            vec![
                (Some("urn:ngsi-ld:Dataset:a"), property(number(1))),
                (Some("urn:ngsi-ld:Dataset:b"), property(number(2))),
            ],
        )]);
        let current = building(vec![(
            TEMPERATURE,
            vec![
                (Some("urn:ngsi-ld:Dataset:a"), property(number(1))),
                (Some("urn:ngsi-ld:Dataset:b"), property(number(3))),
            ],
        )]);

        pretty_assertions::assert_eq!(
            written_observation(Some(&previous), &current),
            Some(building(vec![(TEMPERATURE, vec![(Some("urn:ngsi-ld:Dataset:b"), property(number(3)))])]))
        );
    }

    /// An instance the write appended beside held ones is observed alone.
    #[test]
    fn an_instance_appended_beside_held_ones_is_observed_alone() {
        let previous = building(vec![(TEMPERATURE, vec![(None, property(number(21)))])]);
        let current = building(vec![(
            TEMPERATURE,
            vec![(None, property(number(21))), (Some("urn:ngsi-ld:Dataset:b"), property(number(22)))],
        )]);

        pretty_assertions::assert_eq!(
            written_observation(Some(&previous), &current),
            Some(building(vec![(TEMPERATURE, vec![(Some("urn:ngsi-ld:Dataset:b"), property(number(22)))])]))
        );
    }

    /// A deleted instance is absent from the stored state and is recorded by a
    /// tombstone, never observed.
    #[test]
    fn a_deleted_instance_is_not_observed() {
        let previous = building(vec![
            (TEMPERATURE, vec![(None, property(number(1)))]),
            (NAME, vec![(None, property(text("hall")))]),
        ]);
        let current = building(vec![(NAME, vec![(None, property(text("hall")))])]);

        pretty_assertions::assert_eq!(written_observation(Some(&previous), &current), None);
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

        pretty_assertions::assert_eq!(written_observation(Some(&previous), &current), Some(current));
    }

    /// A creation has no predecessor: every instance and the Scope are
    /// observed, the entity's own `expiresAt` is not an attribute and is not.
    #[test]
    fn a_creation_observes_every_instance_and_the_scope() {
        let mut current = building(vec![
            (
                TEMPERATURE,
                vec![(None, property(number(2))), (Some("urn:ngsi-ld:Dataset:b"), property(number(3)))],
            ),
            (NAME, vec![(None, property(text("lobby")))]),
        ]);
        current.set_scope(Some(scope(&["/Madrid"])));
        current.set_expires_at(Some(date_time("2030-01-01T00:00:00Z")));

        let observed = written_observation(None, &current).expect("an observation");

        let mut expected = current.clone();
        expected.set_expires_at(None);
        pretty_assertions::assert_eq!(observed, expected);
    }

    /// An entity holding neither an attribute nor a Scope observes nothing.
    #[test]
    fn an_empty_entity_observes_nothing() {
        pretty_assertions::assert_eq!(written_observation(None, &building(Vec::new())), None);
    }

    /// The entity's `expiresAt` is no attribute and changing it alone observes
    /// nothing.
    #[test]
    fn a_changed_entity_expiry_is_not_observed() {
        let previous = building(Vec::new());
        let mut current = building(Vec::new());
        current.set_expires_at(Some(date_time("2030-01-01T00:00:00Z")));

        pretty_assertions::assert_eq!(written_observation(Some(&previous), &current), None);
    }

    /// An attribute whose every instance is observed is shared with the entity
    /// rather than copied.
    #[test]
    fn an_attribute_observed_whole_is_shared_with_the_entity() {
        let previous = building(vec![(TEMPERATURE, vec![(None, property(number(20)))])]);
        let current = building(vec![(TEMPERATURE, vec![(None, property(number(21)))])]);

        let observed = written_observation(Some(&previous), &current).expect("an observation");

        let shared = |state: &EntityState| Arc::clone(state.attributes().get_shared(TEMPERATURE).expect("the attribute"));
        assert!(Arc::ptr_eq(&shared(&current), &shared(&observed)));
    }

    /// An attribute the write carried over from the previous state, sharing its
    /// instances, is not even compared and is not observed.
    #[test]
    fn an_attribute_shared_with_the_previous_state_is_not_observed() {
        let previous = building(vec![
            (NAME, vec![(None, property(text("hall")))]),
            (TEMPERATURE, vec![(None, property(number(20)))]),
        ]);
        let mut current = previous.clone();
        current
            .attributes_mut()
            .insert(name(TEMPERATURE), AttributeInstances::one(None, property(number(21))));

        pretty_assertions::assert_eq!(
            written_observation(Some(&previous), &current),
            Some(building(vec![(TEMPERATURE, vec![(None, property(number(21)))])]))
        );
    }

    /// Clause 4.18: a write that leaves the entity in a Scope it did not hold
    /// sets it, so the observation carries it.
    #[test]
    fn a_changed_scope_is_observed() {
        let observed = written_observation(Some(&scoped(&["/Berlin"])), &scoped(&["/Berlin", "/Madrid"]));

        pretty_assertions::assert_eq!(observed.as_ref().and_then(EntityState::scope), Some(&scope(&["/Berlin", "/Madrid"])));
    }

    /// Clause 4.18: a write restating the Scope in force sets nothing, so it
    /// observes nothing.
    #[test]
    fn a_restated_scope_observes_nothing() {
        pretty_assertions::assert_eq!(written_observation(Some(&scoped(&["/Madrid"])), &scoped(&["/Madrid"])), None);
    }

    /// A write modifying an attribute of an entity whose Scope it restated
    /// observes the attribute and no Scope.
    #[test]
    fn a_restated_scope_beside_a_modified_attribute_observes_the_attribute_alone() {
        let mut previous = building(vec![(NAME, vec![(None, property(text("hall")))])]);
        previous.set_scope(Some(scope(&["/Madrid"])));
        let mut current = building(vec![(NAME, vec![(None, property(text("lobby")))])]);
        current.set_scope(Some(scope(&["/Madrid"])));

        let observed = written_observation(Some(&previous), &current).expect("an observation");

        pretty_assertions::assert_eq!(observed.scope(), None);
        pretty_assertions::assert_eq!(observed, building(vec![(NAME, vec![(None, property(text("lobby")))])]));
    }
}
