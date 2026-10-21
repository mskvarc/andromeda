//! Landing the written instances of one attribute on an entity's attributes,
//! one instance at a time, matched by `datasetId` (clause 4.5.5).
//!
//! A written member may mix deletions with data (clauses 5.6.2, 5.6.17):
//! applying the instances in turn is what keeps an update that shares the
//! member with a deletion from being dropped, and stops a `datasetId`-less
//! deletion from swallowing the instances that follow it.

use crate::{
    attribute_instances::{AttributeInstances, InstanceRemoval},
    attributes::Attributes,
    document::{instance_document::InstanceDocument, slot::Slot, written_value::ValueSlot},
    operation::{
        instance_patch::{InstancePatchDepth, new_instance, patch_instance},
        mutation_error::MutationError,
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, time::written_date_time::DateTimeText};

/// Whether removing an instance found one to remove.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InstanceRemovalOutcome {
    /// The instance was removed; the attribute went with its last instance.
    Removed,
    /// No instance had the identity.
    Absent,
}

/// Land every written instance of the attribute `name`, in order.
///
/// An instance whose value is its family's deletion encoding deletes the
/// held instance with its `datasetId` (the default one when it names none),
/// and deletes nothing when there is none. Every other instance is applied to
/// the held instance with its `datasetId` under `depth`, or added.
///
/// `default_observed_at` is Table 6.5.3.4-1's `observedAt`: an instance that
/// names no `observedAt` while the instance it lands on carries one takes it.
///
/// # Errors
///
/// Returns the refusal of a written instance that cannot become state.
pub(crate) fn apply_instances(
    attributes: &mut Attributes,
    name: &AttributeName,
    documents: &[InstanceDocument],
    depth: InstancePatchDepth,
    default_observed_at: Option<&DateTimeText>,
) -> Result<(), MutationError> {
    documents
        .iter()
        .try_for_each(|document| apply_instance(attributes, name, document, depth, default_observed_at))
}

/// Land one written instance of the attribute `name`.
fn apply_instance(
    attributes: &mut Attributes,
    name: &AttributeName,
    document: &InstanceDocument,
    depth: InstancePatchDepth,
    default_observed_at: Option<&DateTimeText>,
) -> Result<(), MutationError> {
    let dataset_id = document.dataset_id();
    if let ValueSlot::Remove(_) = document.value() {
        remove_instance(attributes, name, dataset_id);
        return Ok(());
    }
    let stamp = default_observed_at
        .filter(|_| document.observed_at().is_none() && previously_observed(attributes, name, dataset_id))
        .map(|observed_at| Slot::Set(observed_at.clone())); // clone: each stamped instance owns its copy of the request parameter.
    let Some(instances) = attributes.get_mut(name) else {
        let instance = new_instance(name, document, stamp.as_ref())?;
        // clone: the entity owns the attribute name and the instance identity it gains.
        attributes.insert(name.clone(), AttributeInstances::one(dataset_id.cloned(), instance));
        return Ok(());
    };
    if let Some(held) = instances.get_mut(dataset_id) {
        return patch_instance(name, held, document, depth, stamp.as_ref());
    }
    let instance = new_instance(name, document, stamp.as_ref())?;
    // clone: the instance is keyed by its own copy of the written identity.
    instances.insert(dataset_id.cloned(), instance);
    Ok(())
}

/// Whether the held instance of `name` with `dataset_id` carries an
/// `observedAt`.
fn previously_observed(attributes: &Attributes, name: &str, dataset_id: Option<&Identifier>) -> bool {
    attributes
        .get(name)
        .and_then(|instances| instances.get(dataset_id))
        .is_some_and(|instance| instance.observed_at().is_some())
}

/// Remove the instance of `name` with `dataset_id` (the default instance for
/// `None`), and the attribute with its last instance.
///
/// Only an attribute that keeps other instances is copied when another state
/// shares it.
pub(crate) fn remove_instance(attributes: &mut Attributes, name: &str, dataset_id: Option<&Identifier>) -> InstanceRemovalOutcome {
    match attributes.get(name) {
        Some(instances) if instances.contains(dataset_id) && instances.len() == 1 => {
            attributes.remove(name);
            InstanceRemovalOutcome::Removed
        }
        Some(instances) if instances.contains(dataset_id) => match attributes.get_mut(name).map(|instances| instances.remove(dataset_id)) {
            Some(InstanceRemoval::Removed(_)) => InstanceRemovalOutcome::Removed,
            Some(InstanceRemoval::Emptied(_)) => {
                attributes.remove(name);
                InstanceRemovalOutcome::Removed
            }
            Some(InstanceRemoval::Absent) | None => InstanceRemovalOutcome::Absent,
        },
        Some(_) | None => InstanceRemovalOutcome::Absent,
    }
}

#[cfg(test)]
mod tests {
    use crate::operation::{
        attribute_write::{InstanceRemovalOutcome, remove_instance},
        speed_entity::{SPEED, dataset, entity, instances, number, property, values},
    };

    /// Clause 4.5.5: `datasetId` is what separates the instances of a
    /// multi-attribute, so a removal naming one leaves the others standing.
    #[test]
    fn removing_a_named_dataset_id_drops_only_that_instance() {
        let mut target = entity(vec![(
            SPEED,
            instances(vec![
                (Some("urn:ngsi-ld:Dataset:A"), property(number(10))),
                (Some("urn:ngsi-ld:Dataset:B"), property(number(20))),
                (None, property(number(30))),
            ]),
        )]);
        let outcome = remove_instance(target.attributes_mut(), SPEED, Some(&dataset("urn:ngsi-ld:Dataset:A")));
        pretty_assertions::assert_eq!(outcome, InstanceRemovalOutcome::Removed);
        pretty_assertions::assert_eq!(values(&target, SPEED), vec![20, 30]);
    }

    #[test]
    fn removing_the_default_instance_drops_the_one_carrying_no_dataset_id() {
        let mut target = entity(vec![(
            SPEED,
            instances(vec![(Some("urn:ngsi-ld:Dataset:A"), property(number(10))), (None, property(number(30)))]),
        )]);
        pretty_assertions::assert_eq!(remove_instance(target.attributes_mut(), SPEED, None), InstanceRemovalOutcome::Removed);
        pretty_assertions::assert_eq!(values(&target, SPEED), vec![10]);
    }

    #[test]
    fn removing_an_unmatched_dataset_id_reports_that_nothing_was_removed() {
        let mut target = entity(vec![(SPEED, instances(vec![(Some("urn:ngsi-ld:Dataset:A"), property(number(10)))]))]);
        let outcome = remove_instance(target.attributes_mut(), SPEED, Some(&dataset("urn:ngsi-ld:Dataset:Z")));
        pretty_assertions::assert_eq!(outcome, InstanceRemovalOutcome::Absent);
        pretty_assertions::assert_eq!(values(&target, SPEED), vec![10]);
    }

    #[test]
    fn removing_the_last_instance_drops_the_attribute_member_itself() {
        let mut target = entity(vec![(SPEED, instances(vec![(Some("urn:ngsi-ld:Dataset:A"), property(number(10)))]))]);
        let outcome = remove_instance(target.attributes_mut(), SPEED, Some(&dataset("urn:ngsi-ld:Dataset:A")));
        pretty_assertions::assert_eq!(outcome, InstanceRemovalOutcome::Removed);
        assert!(!target.attributes().contains(SPEED));
    }

    #[test]
    fn removing_from_an_absent_attribute_reports_that_nothing_was_removed() {
        let mut target = entity(Vec::new());
        pretty_assertions::assert_eq!(remove_instance(target.attributes_mut(), SPEED, None), InstanceRemovalOutcome::Absent);
    }
}
