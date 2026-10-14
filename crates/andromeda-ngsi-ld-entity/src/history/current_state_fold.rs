//! Folding attribute instances added to a Temporal Evolution into the entity's
//! current state.
//!
//! A Temporal Evolution (clause 5.2.20) may carry many instances of one
//! attribute under one `datasetId`, while current state holds at most one
//! (clause 4.5.5). The instances a Temporal API write adds therefore reach
//! current state through clause 4.5.5.3's rule for the most current instance:
//! past-due instances are discarded, then the most recent `observedAt` stands,
//! then the most recent `modifiedAt`. The instance being added is the newest
//! write, so it stands whenever no `observedAt` pair says it is older, and a
//! backfill of observations older than current state changes nothing at all.

use crate::{
    attribute_instances::AttributeInstances,
    attributes::Attributes,
    date_time_text::DateTimeText,
    document::{EntityDocument, instance_document::InstanceDocument, slot::Slot},
    entity_state::EntityState,
    entity_types::EntityTypes,
    history::instance_recency::InstanceRecency,
    instance::Instance,
    operation::{attribute_write::remove_instance, instance_patch::new_instance, mutation_error::MutationError},
    system_instant::SystemInstant,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use chrono::{DateTime, Utc};
use std::cmp::Ordering;
use thiserror::Error;

/// A Temporal Evolution fragment that cannot be folded into current state.
#[derive(Debug, Error)]
pub enum CurrentStateFoldError {
    /// The fragment creates the entity but names no identifier.
    #[error("the Temporal Evolution fragment creating an entity carries no '@id'")]
    MissingId,
    /// The fragment creates the entity but names no type (clause 4.5.1).
    #[error("the Temporal Evolution fragment creating an entity carries no type")]
    MissingType,
    /// A standing instance cannot be current state.
    #[error(transparent)]
    Instance(#[from] MutationError),
}

/// Whether a candidate instance stood over the instance current state held.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Standing {
    /// The candidate is now current (or ended the current one).
    Stood,
    /// Clause 4.5.5.3 makes the held instance the more current one.
    Older,
}

/// Fold the attribute instances of `evolution` into `current`, the entity's
/// current state, or into a new entity when there is none, at the write
/// instant `at` (clauses 4.5.5.3, 4.5.7, 5.6.11.4).
///
/// Returns the entity's current state after the fold, or `None` when current
/// state existed and no instance of the fragment stands over what it held. A new entity is always created, with the
/// fragment's identifier, types and scope, even when no instance of the
/// fragment stands. The fragment's types are added to the entity's. The
/// instances keep their members except `instanceId`, which identifies an
/// instance of the Temporal Evolution and has no place in current state
/// (clause 4.5.7).
///
/// # Errors
///
/// Returns [`CurrentStateFoldError::MissingId`] or
/// [`CurrentStateFoldError::MissingType`] when the fragment creates an entity
/// it does not identify, and [`CurrentStateFoldError::Instance`] when a
/// standing instance cannot be state.
pub fn fold_into_current_state(
    current: Option<&EntityState>,
    evolution: &EntityDocument,
    at: DateTime<Utc>,
) -> Result<Option<EntityState>, CurrentStateFoldError> {
    let mut state = match current {
        // clone: the fold builds the new current state from the stored one, sharing every attribute it does not touch.
        Some(current) => current.clone(),
        None => new_entity(evolution)?,
    };
    // clone: the entity owns the types it gains.
    state.types_mut().extend(evolution.types().iter().cloned());
    let mut stood = Standing::Older;
    for (name, member) in evolution.attributes() {
        for candidate in member.instances().iter().filter(|candidate| !expired(candidate, at)) {
            if stand(&mut state, name, candidate, at)? == Standing::Stood {
                stood = Standing::Stood;
            }
        }
    }
    Ok(match (current, stood) {
        (Some(_), Standing::Older) => None,
        (Some(_), Standing::Stood) | (None, Standing::Stood | Standing::Older) => Some(state),
    })
}

/// Let `candidate` stand over the instance `state` holds under its identity
/// when clause 4.5.5.3 does not make it the older one.
///
/// A candidate recording a deletion (it carries `deletedAt`, clause 4.5.7)
/// removes the held instance instead of replacing it.
fn stand(state: &mut EntityState, name: &AttributeName, candidate: &InstanceDocument, at: DateTime<Utc>) -> Result<Standing, MutationError> {
    let dataset_id = candidate.dataset_id();
    let held = state.attributes().get(name).and_then(|instances| instances.get(dataset_id));
    let candidate_recency = InstanceRecency {
        observed_at: candidate.observed_at().and_then(Slot::as_set).map(DateTimeText::instant),
        // The candidate is the newest write, so only an `observedAt` pair can
        // make it the older instance.
        modified_at: Some(at),
    };
    if held.is_some_and(|held| recency_of(held).order(&candidate_recency) == Ordering::Greater) {
        return Ok(Standing::Older);
    }
    match (held, candidate.deleted_at()) {
        (Some(_), Some(_)) => {
            remove_instance(state.attributes_mut(), name, dataset_id);
        }
        (None, Some(_)) => {}
        (Some(_) | None, None) => {
            let instance = new_instance(name, candidate, None)?;
            match state.attributes_mut().get_mut(name) {
                // clone: the instance is keyed by its own copy of the written identity.
                Some(instances) => {
                    instances.insert(dataset_id.cloned(), instance);
                }
                // clone: the entity owns the attribute name and the instance identity it gains.
                None => {
                    state
                        .attributes_mut()
                        .insert(name.clone(), AttributeInstances::one(dataset_id.cloned(), instance));
                }
            }
        }
    }
    Ok(Standing::Stood)
}

/// Whether a candidate is already past its `expiresAt` (clause 4.22).
fn expired(candidate: &InstanceDocument, at: DateTime<Utc>) -> bool {
    candidate
        .expires_at()
        .and_then(Slot::as_set)
        .is_some_and(|expires_at| expires_at.instant() <= at)
}

/// The clause 4.5.5.3 instants a held instance carries.
fn recency_of(instance: &Instance) -> InstanceRecency {
    InstanceRecency {
        observed_at: instance.observed_at().map(DateTimeText::instant),
        modified_at: instance.modified_at().map(SystemInstant::get),
    }
}

/// A new entity with the fragment's identifier, types and scope.
fn new_entity(evolution: &EntityDocument) -> Result<EntityState, CurrentStateFoldError> {
    // clone: the new entity owns its copy of the borrowed fragment's identity.
    let id = evolution.id().cloned().ok_or(CurrentStateFoldError::MissingId)?;
    // clone: the new entity owns its copy of the borrowed fragment's types.
    let types = EntityTypes::try_from_names(evolution.types().iter().cloned()).map_err(|_| CurrentStateFoldError::MissingType)?;
    // clone: the new entity owns its copy of the borrowed fragment's scope.
    let scope = evolution.scope().and_then(Slot::as_set).cloned();
    Ok(EntityState::from_parts(id, types, scope, None, Attributes::new()))
}

#[cfg(test)]
mod tests {
    use crate::{
        document::{
            EntityDocument,
            attribute_member::AttributeMember,
            instance_document::InstanceDocument,
            slot::Slot,
            value_carrier::ValueCarrier,
            written_value::{ValueSlot, WrittenValue},
        },
        entity_state::EntityState,
        history::current_state_fold::fold_into_current_state,
        instance::Instance,
        operation::speed_entity::{dataset, date_time, entity, instances, integer_of, name, number, observed_document, property, property_document},
        system_instant::SystemInstant,
    };
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;

    const V: &str = "https://example.org/v";

    fn at(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("a valid instant").with_timezone(&Utc)
    }

    /// A Temporal Evolution instance carrying `value` and its own
    /// `instanceId`.
    fn instance(value: i64, observed_at: Option<&str>) -> InstanceDocument {
        let mut document = match observed_at {
            Some(observed_at) => observed_document(value, None, observed_at),
            None => property_document(number(value), None),
        };
        document.set_instance_id(Some("urn:ngsi-ld:instance:x".parse().expect("a valid identifier")));
        document
    }

    fn evolution(instances: Vec<InstanceDocument>) -> EntityDocument {
        EntityDocument::new(
            Some("urn:ngsi-ld:TQ:1".parse().expect("a valid identifier")),
            vec!["https://example.org/TQ".parse().expect("a valid type")],
            None,
            None,
            [(name(V), AttributeMember::Instances(instances))].into_iter().collect(),
        )
    }

    fn values(state: &EntityState) -> Vec<i64> {
        state
            .attributes()
            .get(V)
            .expect("the attribute is present")
            .iter()
            .filter_map(|(_, instance)| integer_of(instance))
            .collect()
    }

    /// Clause 4.5.5: a new entity holds one default instance, the one with the
    /// most recent `observedAt`, whatever order the series was written in.
    #[test]
    fn a_new_entity_holds_the_most_recently_observed_instance_alone() {
        let series = evolution(vec![
            instance(10, Some("2020-01-01T00:00:00Z")),
            instance(200, Some("2020-01-03T00:00:00Z")),
            instance(50, Some("2020-01-02T00:00:00Z")),
        ]);
        let fold = fold_into_current_state(None, &series, at("2026-01-01T00:00:00Z"))
            .expect("a foldable fragment")
            .expect("a new entity is always created");
        assert_eq!(values(&fold), vec![200]);
        assert_eq!(fold.types().as_slice(), &["https://example.org/TQ".parse().expect("a valid type")]);
    }

    /// Instances without `observedAt` tie, and the later one stands.
    #[test]
    fn instances_without_observed_at_resolve_to_the_later_one() {
        let series = evolution(vec![instance(1, None), instance(2, None)]);
        let fold = fold_into_current_state(None, &series, at("2026-01-01T00:00:00Z"))
            .expect("foldable")
            .expect("created");
        assert_eq!(values(&fold), vec![2]);
    }

    /// A newer observation replaces the current instance; a backfill of an
    /// older one changes nothing.
    #[test]
    fn a_newer_observation_stands_and_a_backfill_changes_nothing() {
        let mut held = property(number(5));
        held.set_observed_at(Some(date_time("2021-01-01T00:00:00Z")));
        held.set_modified_at(Some("2021-01-01T00:00:00Z".parse::<SystemInstant>().expect("a valid instant")));
        let current = entity(vec![(V, instances(vec![(None, held)]))]);

        let newer = fold_into_current_state(
            Some(&current),
            &evolution(vec![instance(7, Some("2022-01-01T00:00:00Z"))]),
            at("2026-01-01T00:00:00Z"),
        )
        .expect("foldable")
        .expect("the newer observation stands");
        assert_eq!(values(&newer), vec![7]);

        let backfill = fold_into_current_state(
            Some(&current),
            &evolution(vec![instance(1, Some("2019-01-01T00:00:00Z"))]),
            at("2026-01-01T00:00:00Z"),
        )
        .expect("foldable");
        assert!(backfill.is_none(), "an older observation leaves current state as it was");
    }

    /// Each `datasetId` is its own instance of the attribute.
    #[test]
    fn each_dataset_keeps_its_own_standing_instance() {
        let named = observed_document(9, Some("urn:ngsi-ld:Dataset:x"), "2020-01-01T00:00:00Z");
        let series = evolution(vec![instance(1, Some("2020-01-02T00:00:00Z")), named]);
        let fold = fold_into_current_state(None, &series, at("2026-01-01T00:00:00Z"))
            .expect("foldable")
            .expect("created");
        let mut held = values(&fold);
        held.sort_unstable();
        assert_eq!(held, vec![1, 9]);
        assert!(
            fold.attributes()
                .get(V)
                .is_some_and(|instances| instances.contains(Some(&dataset("urn:ngsi-ld:Dataset:x"))))
        );
    }

    /// Clause 4.22: an instance already past its `expiresAt` never stands.
    #[test]
    fn an_expired_instance_never_stands() {
        let expired = InstanceDocument::builder()
            .family(Some(AttributeType::Property))
            .value(ValueSlot::Set(WrittenValue::Value(vec![number(99)])))
            .observed_at(Some(Slot::Set(date_time("2025-01-01T00:00:00Z"))))
            .expires_at(Some(Slot::Set(date_time("2025-06-01T00:00:00Z"))))
            .build();
        let series = evolution(vec![instance(1, Some("2020-01-01T00:00:00Z")), expired]);
        let fold = fold_into_current_state(None, &series, at("2026-01-01T00:00:00Z"))
            .expect("foldable")
            .expect("created");
        assert_eq!(values(&fold), vec![1]);
    }

    /// A standing deletion instance ends the current one.
    #[test]
    fn a_standing_deletion_removes_the_current_instance() {
        let mut held: Instance = property(number(5));
        held.set_observed_at(Some(date_time("2021-01-01T00:00:00Z")));
        let current = entity(vec![(V, instances(vec![(None, held)]))]);
        let deletion = InstanceDocument::builder()
            .family(Some(AttributeType::Property))
            .value(ValueSlot::Remove(ValueCarrier::Value))
            .observed_at(Some(Slot::Set(date_time("2022-01-01T00:00:00Z"))))
            .deleted_at(Some(date_time("2022-01-01T00:00:00Z")))
            .build();

        let fold = fold_into_current_state(Some(&current), &evolution(vec![deletion]), at("2026-01-01T00:00:00Z"))
            .expect("foldable")
            .expect("the deletion stands");

        assert!(!fold.attributes().contains(V));
    }
}
