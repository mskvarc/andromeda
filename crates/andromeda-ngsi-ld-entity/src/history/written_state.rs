//! A whole entity a write stores, as Create Entity (clause 5.6.1) and Replace
//! Entity (clause 5.6.18) write it: the current state it holds and the
//! observation it records for the Temporal Evolution.
//!
//! Table 5.2.5-2 has `instanceId` "Only used in temporal representation of
//! Properties". An `instanceId` a written instance carries names the instance
//! the write adds to the Temporal Evolution (clause 4.5.7), so the observation
//! keeps it and the current state holds none.

use crate::{attribute_instances::AttributeInstances, attributes::Attributes, entity_state::EntityState};
use std::sync::Arc;

/// A written entity, split into what the write stores and what it observes.
#[derive(Clone, Debug, PartialEq)]
pub struct WrittenState {
    /// The current state the write stores, with no `instanceId` on any
    /// instance or sub-attribute instance.
    pub held: Arc<EntityState>,
    /// The state the write records as its observation, with every
    /// `instanceId` the request supplied.
    pub observed: Arc<EntityState>,
}

impl WrittenState {
    /// Split `written`. The two share `written` when no instance carries an
    /// `instanceId`; otherwise only the attributes that carry one are copied.
    #[must_use]
    pub fn of(written: Arc<EntityState>) -> WrittenState {
        if !attributes_carry_instance_id(written.attributes()) {
            return WrittenState {
                held: Arc::clone(&written), // clone: reference-count bump; the state is also the observation
                observed: written,
            };
        }
        // clone: the stored state differs from the observation in the identities it holds; untouched attributes stay shared.
        let mut held = EntityState::clone(&written);
        clear_instance_ids(held.attributes_mut());
        WrittenState {
            held: Arc::new(held),
            observed: written,
        }
    }
}

/// Whether an instance of `attributes`, or of their sub-attributes, carries an
/// `instanceId`.
fn attributes_carry_instance_id(attributes: &Attributes) -> bool {
    attributes.iter().any(|(_, instances)| instances_carry_instance_id(instances))
}

/// Whether an instance of one attribute, or of its sub-attributes, carries an
/// `instanceId`.
fn instances_carry_instance_id(instances: &AttributeInstances) -> bool {
    instances
        .iter()
        .any(|(_, instance)| instance.instance_id().is_some() || attributes_carry_instance_id(instance.sub_attributes()))
}

/// Remove every `instanceId` from `attributes`, copying only the attributes
/// that carry one.
fn clear_instance_ids(attributes: &mut Attributes) {
    for (_, instances) in attributes.iter_shared_mut() {
        if !instances_carry_instance_id(instances) {
            continue;
        }
        for (_, instance) in Arc::make_mut(instances).iter_mut() {
            instance.set_instance_id(None);
            clear_instance_ids(instance.sub_attributes_mut());
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        history::written_state::WrittenState,
        instance::Instance,
        operation::speed_entity::{SPEED, entity, instances, name, number, property},
    };
    use std::sync::Arc;

    /// A `speed` instance carrying the given `instanceId`.
    fn identified(instance_id: &str) -> Instance {
        let mut instance = property(number(10));
        instance.set_instance_id(Some(instance_id.parse().expect("a valid identifier")));
        instance
    }

    #[test]
    fn a_state_with_no_instance_id_is_stored_and_observed_as_one() {
        let written = Arc::new(entity(vec![(SPEED, instances(vec![(None, property(number(10)))]))]));

        let split = WrittenState::of(Arc::clone(&written));

        assert!(Arc::ptr_eq(&split.held, &written));
        assert!(Arc::ptr_eq(&split.observed, &written));
    }

    #[test]
    fn the_stored_state_drops_an_instance_id_the_observation_keeps() {
        let written = Arc::new(entity(vec![(SPEED, instances(vec![(None, identified("urn:ngsi-ld:Instance:1"))]))]));

        let split = WrittenState::of(Arc::clone(&written));

        let held = split.held.attributes().get(SPEED).and_then(|held| held.get(None)).expect("the instance");
        pretty_assertions::assert_eq!(held.instance_id(), None);
        assert!(Arc::ptr_eq(&split.observed, &written));
    }

    #[test]
    fn the_stored_state_drops_an_instance_id_on_a_sub_attribute() {
        let mut carrier = property(number(20));
        carrier
            .sub_attributes_mut()
            .insert(name(SPEED), instances(vec![(None, identified("urn:ngsi-ld:Instance:2"))]));
        let written = Arc::new(entity(vec![("https://example.com/temperature", instances(vec![(None, carrier)]))]));

        let split = WrittenState::of(written);

        let sub = split
            .held
            .attributes()
            .get("https://example.com/temperature")
            .and_then(|held| held.get(None))
            .and_then(|instance| instance.sub_attributes().get(SPEED))
            .and_then(|held| held.get(None))
            .expect("the sub-attribute instance");
        pretty_assertions::assert_eq!(sub.instance_id(), None);
    }
}
