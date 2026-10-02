//! The clause 4.8 system times of attribute instances.
//!
//! Clause 4.8 defines `createdAt` and `modifiedAt` for "the Entity, Property
//! or Relationship", and Table 5.2.14-1 includes them in a representation the
//! request asked for with `sysAttrs`. The entity-level pair lives in store
//! columns; the attribute-instance pair lives on each instance, at the grain
//! of one instance per `datasetId` (clause 4.5.5). A write stamps them
//! ([`EntityState::stamp_attribute_instants`]) and a representation that did
//! not ask for `sysAttrs` drops them
//! ([`EntityState::strip_attribute_instants`]).

use crate::{attribute_instances::AttributeInstances, entity_state::EntityState, instance::Instance, system_instant::SystemInstant};
use std::sync::Arc;

impl EntityState {
    /// Stamp `createdAt` and `modifiedAt` onto every attribute instance,
    /// given the entity before the write (`None` when the write creates it)
    /// and the write instant (clause 4.8).
    ///
    /// The rule is per instance, matched to its predecessor by `datasetId`:
    ///
    /// - an instance with no predecessor is new: both times take `now`;
    /// - an instance whose content (everything but the system times) equals
    ///   its predecessor's keeps both of the predecessor's times, so a
    ///   restatement that leaves an instance untouched (an Append with
    ///   `noOverwrite`, say) does not move its `modifiedAt`;
    /// - an instance whose content changed keeps the predecessor's
    ///   `createdAt` and takes `now` for `modifiedAt`.
    ///
    /// Keeping the predecessor's `createdAt` is what lets it survive a write
    /// that replaces the whole instance (clauses 5.6.2, 5.6.19). An attribute
    /// is copied only when one of its stamps actually changes; one this
    /// state still shares with `previous` holds the predecessor's instances
    /// themselves and is not even compared.
    pub fn stamp_attribute_instants(&mut self, previous: Option<&EntityState>, now: SystemInstant) {
        for (name, instances) in self.attributes_mut().iter_shared_mut() {
            let predecessors = previous.and_then(|previous| previous.attributes().get_shared(name));
            let stamps_of = |dataset_id, instance: &Instance| match predecessors {
                // The very instances of the previous state: their own stamps
                // are the predecessor's.
                Some(predecessors) if Arc::ptr_eq(predecessors, instances) => (instance.created_at().unwrap_or(now), instance.modified_at().unwrap_or(now)),
                Some(predecessors) => stamps(instance, predecessors.get(dataset_id), now),
                None => stamps(instance, None, now),
            };
            let unchanged = instances.iter().all(|(dataset_id, instance)| {
                let (created_at, modified_at) = stamps_of(dataset_id, instance);
                instance.created_at() == Some(created_at) && instance.modified_at() == Some(modified_at)
            });
            if unchanged {
                continue;
            }
            let predecessors = predecessors.map(Arc::as_ref);
            for (dataset_id, instance) in Arc::make_mut(instances).iter_mut() {
                let (created_at, modified_at) = stamps(instance, predecessors.and_then(|predecessors| predecessors.get(dataset_id)), now);
                instance.set_created_at(Some(created_at));
                instance.set_modified_at(Some(modified_at));
            }
        }
    }

    /// Remove `createdAt` and `modifiedAt` from every attribute instance, for
    /// a representation the request did not ask to carry `sysAttrs` (Table
    /// 5.2.14-1). Only attributes carrying a stamp are copied.
    pub fn strip_attribute_instants(&mut self) {
        for (_, instances) in self.attributes_mut().iter_shared_mut() {
            if !carries_a_stamp(instances) {
                continue;
            }
            for (_, instance) in Arc::make_mut(instances).iter_mut() {
                instance.set_created_at(None);
                instance.set_modified_at(None);
            }
        }
    }
}

/// The `(createdAt, modifiedAt)` an instance takes given its predecessor.
fn stamps(instance: &Instance, predecessor: Option<&Instance>, now: SystemInstant) -> (SystemInstant, SystemInstant) {
    match predecessor {
        None => (now, now),
        Some(predecessor) => {
            let created_at = predecessor.created_at().unwrap_or(now);
            let modified_at = if instance.same_content(predecessor) {
                predecessor.modified_at().unwrap_or(now)
            } else {
                now
            };
            (created_at, modified_at)
        }
    }
}

/// Whether an instance of the attribute carries a system time.
fn carries_a_stamp(instances: &AttributeInstances) -> bool {
    instances
        .iter()
        .any(|(_, instance)| instance.created_at().is_some() || instance.modified_at().is_some())
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_state::EntityState,
        instance::Instance,
        operation::speed_entity::{SPEED, dataset, entity, instances, number, property},
        system_instant::SystemInstant,
    };
    use std::sync::Arc;

    /// An instant parsed from an RFC 3339 string.
    fn at(value: &str) -> SystemInstant {
        value.parse().expect("a valid instant")
    }

    /// A stored Property instance stamped with the given times.
    fn stamped(value: i64, created_at: &str, modified_at: &str) -> Instance {
        let mut instance = property(number(value));
        instance.set_created_at(Some(at(created_at)));
        instance.set_modified_at(Some(at(modified_at)));
        instance
    }

    /// The stamps of one `speed` instance, as text.
    fn stamps_of(entity: &EntityState, dataset_id: Option<&str>) -> (Option<String>, Option<String>) {
        let dataset_id = dataset_id.map(dataset);
        let instance = entity
            .attributes()
            .get(SPEED)
            .and_then(|held| held.get(dataset_id.as_ref()))
            .expect("the instance");
        (
            instance.created_at().map(|stamp| stamp.to_string()),
            instance.modified_at().map(|stamp| stamp.to_string()),
        )
    }

    fn texts(created_at: &str, modified_at: &str) -> (Option<String>, Option<String>) {
        (Some(created_at.to_owned()), Some(modified_at.to_owned()))
    }

    #[test]
    fn a_created_instance_takes_now_for_both_times() {
        let mut target = entity(vec![(SPEED, instances(vec![(None, property(number(10)))]))]);

        target.stamp_attribute_instants(None, at("2024-02-03T04:05:06Z"));

        pretty_assertions::assert_eq!(stamps_of(&target, None), texts("2024-02-03T04:05:06.000000Z", "2024-02-03T04:05:06.000000Z"));
    }

    #[test]
    fn a_changed_instance_keeps_created_and_moves_modified() {
        let previous = entity(vec![(
            SPEED,
            instances(vec![(None, stamped(10, "2020-01-01T00:00:00Z", "2020-01-01T00:00:00Z"))]),
        )]);
        let mut target = entity(vec![(SPEED, instances(vec![(None, property(number(20)))]))]);

        target.stamp_attribute_instants(Some(&previous), at("2024-02-03T04:05:06Z"));

        pretty_assertions::assert_eq!(stamps_of(&target, None), texts("2020-01-01T00:00:00.000000Z", "2024-02-03T04:05:06.000000Z"));
    }

    #[test]
    fn an_unchanged_instance_keeps_both_times() {
        let previous = entity(vec![(
            SPEED,
            instances(vec![(None, stamped(10, "2020-01-01T00:00:00Z", "2021-06-01T00:00:00Z"))]),
        )]);
        let mut target = entity(vec![(SPEED, instances(vec![(None, property(number(10)))]))]);

        target.stamp_attribute_instants(Some(&previous), at("2024-02-03T04:05:06Z"));

        pretty_assertions::assert_eq!(
            stamps_of(&target, None),
            texts("2020-01-01T00:00:00.000000Z", "2021-06-01T00:00:00.000000Z"),
            "an untouched instance does not move its modifiedAt"
        );
    }

    #[test]
    fn each_dataset_instance_keeps_its_own_times() {
        let previous = entity(vec![(
            SPEED,
            instances(vec![(
                Some("urn:ngsi-ld:Dataset:A"),
                stamped(10, "2020-01-01T00:00:00Z", "2020-01-01T00:00:00Z"),
            )]),
        )]);
        // Dataset A is restated unchanged; dataset B is new.
        let mut target = entity(vec![(
            SPEED,
            instances(vec![
                (Some("urn:ngsi-ld:Dataset:A"), property(number(10))),
                (Some("urn:ngsi-ld:Dataset:B"), property(number(30))),
            ]),
        )]);

        target.stamp_attribute_instants(Some(&previous), at("2024-02-03T04:05:06Z"));

        pretty_assertions::assert_eq!(
            stamps_of(&target, Some("urn:ngsi-ld:Dataset:A")).0.as_deref(),
            Some("2020-01-01T00:00:00.000000Z")
        );
        pretty_assertions::assert_eq!(
            stamps_of(&target, Some("urn:ngsi-ld:Dataset:B")).0.as_deref(),
            Some("2024-02-03T04:05:06.000000Z")
        );
    }

    #[test]
    fn a_strip_removes_every_instance_system_time() {
        let mut target = entity(vec![(SPEED, instances(vec![(None, property(number(10)))]))]);
        target.stamp_attribute_instants(None, at("2024-02-03T04:05:06Z"));

        target.strip_attribute_instants();

        pretty_assertions::assert_eq!(stamps_of(&target, None), (None, None));
    }

    /// Stamping a state derived from the previous one copies only the
    /// attributes whose stamps change: an attribute both states share is not
    /// copied.
    #[test]
    fn stamping_never_copies_an_unchanged_attribute() {
        let previous = entity(vec![
            (SPEED, instances(vec![(None, stamped(10, "2020-01-01T00:00:00Z", "2020-01-01T00:00:00Z"))])),
            (
                "https://example.com/brand",
                instances(vec![(None, stamped(1, "2020-01-01T00:00:00Z", "2020-01-01T00:00:00Z"))]),
            ),
        ]);
        let mut target = previous.clone();
        if let Some(speed) = target.attributes_mut().get_mut(SPEED)
            && let Some(instance) = speed.get_mut(None)
        {
            *instance = property(number(20));
        }

        target.stamp_attribute_instants(Some(&previous), at("2024-02-03T04:05:06Z"));

        let shared = |state: &EntityState, attribute: &str| Arc::clone(state.attributes().get_shared(attribute).expect("the attribute"));
        assert!(Arc::ptr_eq(
            &shared(&previous, "https://example.com/brand"),
            &shared(&target, "https://example.com/brand")
        ));
        pretty_assertions::assert_eq!(stamps_of(&target, None), texts("2020-01-01T00:00:00.000000Z", "2024-02-03T04:05:06.000000Z"));
    }

    /// An instance restated with its content unchanged keeps the very stamps
    /// it had, and its attribute is not copied a second time.
    #[test]
    fn stamping_an_unchanged_restatement_copies_nothing() {
        let previous = entity(vec![(
            SPEED,
            instances(vec![(None, stamped(10, "2020-01-01T00:00:00Z", "2021-01-01T00:00:00Z"))]),
        )]);
        let mut target = entity(vec![(
            SPEED,
            instances(vec![(None, stamped(10, "2020-01-01T00:00:00Z", "2021-01-01T00:00:00Z"))]),
        )]);
        let before = Arc::clone(target.attributes().get_shared(SPEED).expect("the attribute"));

        target.stamp_attribute_instants(Some(&previous), at("2024-02-03T04:05:06Z"));

        assert!(Arc::ptr_eq(&before, target.attributes().get_shared(SPEED).expect("the attribute")));
    }
}
