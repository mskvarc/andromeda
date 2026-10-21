//! The clause 4.8 system times of attribute instances.
//!
//! Clause 4.8 defines `createdAt` and `modifiedAt` for "the Entity, Property
//! or Relationship", and Table 5.2.14-1 includes them in a representation the
//! request asked for with `sysAttrs`. A sub-attribute is a Property or a
//! Relationship of its instance (clause 4.5.2), so it carries them too. The
//! entity-level pair lives in store columns; the attribute-instance pair lives
//! on each instance, at every depth, at the grain of one instance per
//! `datasetId` (clause 4.5.5). A write stamps them
//! ([`EntityState::stamp_attribute_instants`]) and a representation that did
//! not ask for `sysAttrs` drops them
//! ([`EntityState::strip_attribute_instants`]).

use crate::{attribute_instances::AttributeInstances, attributes::Attributes, entity_state::EntityState, instance::Instance};
use andromeda_ngsi_ld_lexicon::time::system_instant::SystemInstant;
use std::sync::Arc;

impl EntityState {
    /// Stamp `createdAt` and `modifiedAt` onto every attribute instance at
    /// every depth, given the entity before the write (`None` when the write
    /// creates it) and the write instant (clause 4.8).
    ///
    /// The rule is per instance, matched to its predecessor by attribute name
    /// and `datasetId` under the predecessor of its parent:
    ///
    /// - an instance with no predecessor is new: both times take `now`;
    /// - an instance whose content (everything but the system times, at every
    ///   depth) equals its predecessor's keeps both of the predecessor's
    ///   times, so a restatement that leaves an instance untouched (an Append
    ///   with `noOverwrite`, say) does not move its `modifiedAt`;
    /// - an instance whose content changed keeps the predecessor's
    ///   `createdAt` and takes `now` for `modifiedAt`. A changed sub-attribute
    ///   changes the content of the instance holding it.
    ///
    /// Times a client wrote are never kept (clause 5.2.5): every instance
    /// takes the times its predecessor or the write gives it. Keeping the
    /// predecessor's `createdAt` is what lets it survive a write that
    /// replaces the whole instance (clauses 5.6.2, 5.6.19). An attribute is
    /// copied only when one of its stamps actually changes; one this state
    /// still shares with `previous` holds the predecessor's instances
    /// themselves and is not even compared.
    pub fn stamp_attribute_instants(&mut self, previous: Option<&EntityState>, now: SystemInstant) {
        stamp_attributes(self.attributes_mut(), previous.map(EntityState::attributes), now);
    }

    /// Remove `createdAt` and `modifiedAt` from every attribute instance at
    /// every depth, for a representation the request did not ask to carry
    /// `sysAttrs` (Table 5.2.14-1). Only attributes carrying a stamp are
    /// copied.
    pub fn strip_attribute_instants(&mut self) {
        strip_attributes(self.attributes_mut());
    }
}

/// Stamp every instance of `attributes`, the attributes `previous` held before
/// the write.
fn stamp_attributes(attributes: &mut Attributes, previous: Option<&Attributes>, now: SystemInstant) {
    for (name, instances) in attributes.iter_shared_mut() {
        let predecessors = previous.and_then(|previous| previous.get_shared(name));
        let held = match predecessors {
            // The very instances of the previous state: their own stamps are
            // the predecessor's.
            Some(predecessors) if Arc::ptr_eq(predecessors, instances) => fully_stamped(instances),
            Some(predecessors) => stamps_hold(instances, Some(predecessors), now),
            None => stamps_hold(instances, None, now),
        };
        if held {
            continue;
        }
        let predecessors = predecessors.map(Arc::as_ref);
        for (dataset_id, instance) in Arc::make_mut(instances).iter_mut() {
            stamp_instance(instance, predecessors.and_then(|predecessors| predecessors.get(dataset_id)), now);
        }
    }
}

/// Stamp one instance and its sub-attributes.
fn stamp_instance(instance: &mut Instance, predecessor: Option<&Instance>, now: SystemInstant) {
    let (created_at, modified_at) = stamps(instance, predecessor, now);
    instance.set_created_at(Some(created_at));
    instance.set_modified_at(Some(modified_at));
    stamp_attributes(instance.sub_attributes_mut(), predecessor.map(Instance::sub_attributes), now);
}

/// Whether every instance of `instances` already carries, at every depth, the
/// stamps its predecessor among `predecessors` gives it.
fn stamps_hold(instances: &AttributeInstances, predecessors: Option<&AttributeInstances>, now: SystemInstant) -> bool {
    instances.iter().all(|(dataset_id, instance)| {
        let predecessor = predecessors.and_then(|predecessors| predecessors.get(dataset_id));
        let (created_at, modified_at) = stamps(instance, predecessor, now);
        instance.created_at() == Some(created_at)
            && instance.modified_at() == Some(modified_at)
            && attribute_stamps_hold(instance.sub_attributes(), predecessor.map(Instance::sub_attributes), now)
    })
}

/// Whether every instance of `attributes` already carries, at every depth, the
/// stamps the attributes `previous` held give it.
fn attribute_stamps_hold(attributes: &Attributes, previous: Option<&Attributes>, now: SystemInstant) -> bool {
    attributes.iter_shared().all(|(name, instances)| {
        let predecessors = previous.and_then(|previous| previous.get_shared(name));
        match predecessors {
            Some(predecessors) if Arc::ptr_eq(predecessors, instances) => fully_stamped(instances),
            Some(predecessors) => stamps_hold(instances, Some(predecessors), now),
            None => stamps_hold(instances, None, now),
        }
    })
}

/// Whether every instance of `instances` carries both times, at every depth.
fn fully_stamped(instances: &AttributeInstances) -> bool {
    instances.iter().all(|(_, instance)| {
        instance.created_at().is_some() && instance.modified_at().is_some() && instance.sub_attributes().iter().all(|(_, held)| fully_stamped(held))
    })
}

/// Remove the system times of every instance of `attributes`, at every depth.
fn strip_attributes(attributes: &mut Attributes) {
    for (_, instances) in attributes.iter_shared_mut() {
        if !carries_a_stamp(instances) {
            continue;
        }
        for (_, instance) in Arc::make_mut(instances).iter_mut() {
            instance.set_created_at(None);
            instance.set_modified_at(None);
            strip_attributes(instance.sub_attributes_mut());
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

/// Whether an instance of the attribute carries a system time, at any depth.
fn carries_a_stamp(instances: &AttributeInstances) -> bool {
    instances.iter().any(|(_, instance)| {
        instance.created_at().is_some() || instance.modified_at().is_some() || instance.sub_attributes().iter().any(|(_, held)| carries_a_stamp(held))
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_instances::AttributeInstances,
        entity_state::EntityState,
        instance::Instance,
        operation::speed_entity::{SPEED, dataset, entity, instances, name, number, property},
    };
    use andromeda_ngsi_ld_lexicon::time::system_instant::SystemInstant;
    use std::sync::Arc;

    /// The sub-attribute the nested fixtures carry.
    const ACCURACY: &str = "https://example.com/accuracy";

    /// The sub-attribute nested under [`ACCURACY`].
    const METHOD: &str = "https://example.com/method";

    /// An instant parsed from an RFC 3339 string.
    fn at(value: &str) -> SystemInstant {
        value.parse().expect("a valid instant")
    }

    /// `instance` carrying `sub_attribute` as the default instance of `raw`.
    fn holding(mut instance: Instance, raw: &str, sub_attribute: Instance) -> Instance {
        instance.sub_attributes_mut().insert(name(raw), AttributeInstances::one(None, sub_attribute));
        instance
    }

    /// The stamps of the instance at `path` under the default `speed`
    /// instance, as text.
    fn nested_stamps_of(entity: &EntityState, path: &[&str]) -> (Option<String>, Option<String>) {
        let top = entity.attributes().get(SPEED).and_then(|held| held.get(None)).expect("the instance");
        let instance = path.iter().fold(top, |instance, raw| {
            instance.sub_attributes().get(raw).and_then(|held| held.get(None)).expect("the sub-attribute")
        });
        (
            instance.created_at().map(|stamp| stamp.to_string()),
            instance.modified_at().map(|stamp| stamp.to_string()),
        )
    }

    /// `instance` stamped with the given times.
    fn stamped_as(mut instance: Instance, created_at: &str, modified_at: &str) -> Instance {
        instance.set_created_at(Some(at(created_at)));
        instance.set_modified_at(Some(at(modified_at)));
        instance
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

        pretty_assertions::assert_eq!(stamps_of(&target, None), texts("2024-02-03T04:05:06Z", "2024-02-03T04:05:06Z"));
    }

    #[test]
    fn a_changed_instance_keeps_created_and_moves_modified() {
        let previous = entity(vec![(
            SPEED,
            instances(vec![(None, stamped(10, "2020-01-01T00:00:00Z", "2020-01-01T00:00:00Z"))]),
        )]);
        let mut target = entity(vec![(SPEED, instances(vec![(None, property(number(20)))]))]);

        target.stamp_attribute_instants(Some(&previous), at("2024-02-03T04:05:06Z"));

        pretty_assertions::assert_eq!(stamps_of(&target, None), texts("2020-01-01T00:00:00Z", "2024-02-03T04:05:06Z"));
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
            texts("2020-01-01T00:00:00Z", "2021-06-01T00:00:00Z"),
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
        let mut target = entity(vec![(
            SPEED,
            instances(vec![
                (Some("urn:ngsi-ld:Dataset:A"), property(number(10))),
                (Some("urn:ngsi-ld:Dataset:B"), property(number(30))),
            ]),
        )]);

        target.stamp_attribute_instants(Some(&previous), at("2024-02-03T04:05:06Z"));

        pretty_assertions::assert_eq!(stamps_of(&target, Some("urn:ngsi-ld:Dataset:A")).0.as_deref(), Some("2020-01-01T00:00:00Z"));
        pretty_assertions::assert_eq!(stamps_of(&target, Some("urn:ngsi-ld:Dataset:B")).0.as_deref(), Some("2024-02-03T04:05:06Z"));
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
        pretty_assertions::assert_eq!(stamps_of(&target, None), texts("2020-01-01T00:00:00Z", "2024-02-03T04:05:06Z"));
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

    /// A sub-attribute added to an existing instance is new: both of its
    /// times take the write instant, and the instance holding it changed.
    #[test]
    fn a_new_sub_attribute_gets_equal_times() {
        let previous = entity(vec![(
            SPEED,
            instances(vec![(None, stamped_as(property(number(10)), "2020-01-01T00:00:00Z", "2020-01-01T00:00:00Z"))]),
        )]);
        let mut target = entity(vec![(
            SPEED,
            instances(vec![(None, holding(property(number(10)), ACCURACY, property(number(1))))]),
        )]);

        target.stamp_attribute_instants(Some(&previous), at("2024-02-03T04:05:06Z"));

        pretty_assertions::assert_eq!(nested_stamps_of(&target, &[ACCURACY]), texts("2024-02-03T04:05:06Z", "2024-02-03T04:05:06Z"));
        pretty_assertions::assert_eq!(
            nested_stamps_of(&target, &[]),
            texts("2020-01-01T00:00:00Z", "2024-02-03T04:05:06Z"),
            "clause 4.8: the Property changed when its sub-attribute was added"
        );
    }

    /// Clause 5.2.5: `createdAt` and `modifiedAt` are set by the broker, so a
    /// time a client wrote on a sub-attribute is replaced at every depth.
    #[test]
    fn client_times_are_dropped_at_every_depth() {
        let written = |instance: Instance| stamped_as(instance, "1999-01-01T00:00:00Z", "1999-01-01T00:00:00Z");
        let accuracy = holding(written(property(number(1))), METHOD, written(property(number(2))));
        let mut target = entity(vec![(SPEED, instances(vec![(None, holding(property(number(10)), ACCURACY, accuracy))]))]);

        target.stamp_attribute_instants(None, at("2024-02-03T04:05:06Z"));

        for path in [&[ACCURACY][..], &[ACCURACY, METHOD][..]] {
            pretty_assertions::assert_eq!(
                nested_stamps_of(&target, path),
                texts("2024-02-03T04:05:06Z", "2024-02-03T04:05:06Z"),
                "{path:?}"
            );
        }
    }

    /// A changed sub-attribute takes the write instant for `modifiedAt` and
    /// keeps its `createdAt`; the instance holding it changed too, and an
    /// untouched sibling keeps both times.
    #[test]
    fn sub_attribute_stamping_recurses_into_changed_instances() {
        let old = |value| stamped_as(property(number(value)), "2020-01-01T00:00:00Z", "2021-01-01T00:00:00Z");
        let mut stored = holding(old(10), ACCURACY, old(1));
        stored = holding(stored, METHOD, old(7));
        let previous = entity(vec![(SPEED, instances(vec![(None, stored)]))]);
        let written = holding(holding(property(number(10)), ACCURACY, property(number(2))), METHOD, property(number(7)));
        let mut target = entity(vec![(SPEED, instances(vec![(None, written)]))]);

        target.stamp_attribute_instants(Some(&previous), at("2024-02-03T04:05:06Z"));

        pretty_assertions::assert_eq!(nested_stamps_of(&target, &[ACCURACY]), texts("2020-01-01T00:00:00Z", "2024-02-03T04:05:06Z"));
        pretty_assertions::assert_eq!(nested_stamps_of(&target, &[]), texts("2020-01-01T00:00:00Z", "2024-02-03T04:05:06Z"));
        pretty_assertions::assert_eq!(
            nested_stamps_of(&target, &[METHOD]),
            texts("2020-01-01T00:00:00Z", "2021-01-01T00:00:00Z"),
            "the untouched sibling keeps both times"
        );
    }

    /// A restatement that leaves every sub-attribute as it was moves no time
    /// at any depth, although it carries none of the stored times.
    #[test]
    fn an_unchanged_restatement_keeps_every_sub_attribute_time() {
        let old = |value| stamped_as(property(number(value)), "2020-01-01T00:00:00Z", "2021-01-01T00:00:00Z");
        let previous = entity(vec![(SPEED, instances(vec![(None, holding(old(10), ACCURACY, old(1)))]))]);
        let mut target = entity(vec![(
            SPEED,
            instances(vec![(None, holding(property(number(10)), ACCURACY, property(number(1))))]),
        )]);

        target.stamp_attribute_instants(Some(&previous), at("2024-02-03T04:05:06Z"));

        for path in [&[][..], &[ACCURACY][..]] {
            pretty_assertions::assert_eq!(
                nested_stamps_of(&target, path),
                texts("2020-01-01T00:00:00Z", "2021-01-01T00:00:00Z"),
                "{path:?}"
            );
        }
    }

    #[test]
    fn a_strip_removes_sub_attribute_times() {
        let mut target = entity(vec![(
            SPEED,
            instances(vec![(None, holding(property(number(10)), ACCURACY, property(number(1))))]),
        )]);
        target.stamp_attribute_instants(None, at("2024-02-03T04:05:06Z"));

        target.strip_attribute_instants();

        pretty_assertions::assert_eq!(nested_stamps_of(&target, &[ACCURACY]), (None, None));
    }
}
