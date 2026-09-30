//! Committing instances added to a Temporal Evolution together with the
//! current-state change they make.
//!
//! Clauses 5.6.11.4 and 5.6.12.4 add every instance "to the target Temporal
//! Evolution of an Entity", so all of them are history. Current state holds
//! one instance per Attribute instance (clause 4.5.5), the most current one
//! (clause 4.5.5.3), so it takes only the instances that stand over what it
//! held: a Temporal Evolution created for an unknown identifier brings the
//! Entity into being with its most current instances, a newer observation
//! advances it, and a backfill of older ones leaves it as it was. The Entity's
//! document and the projection its `q` filters read change together, in the
//! transaction that records the history.
//!
//! Subscriptions watch current state (clause 5.8.6 defines a change on it
//! alone), so a change the fold makes owes the notifications a Core write
//! making the same change would, committed with it; a backfill owes none.

use crate::{error::TemporalError, service::TemporalService, temporal_invariant::TemporalInvariant, temporal_validation_reason::TemporalValidationReason};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    change_event::ChangeEvent,
    current_state_projection::CurrentStateProjection,
    entity_mutation::EntityMutation,
    entity_record::EntityRecord,
    history_record::HistoryRecord,
    precondition::{AfterAttempt, Precondition, after_attempt},
    submission_refusal::SubmissionRefusal,
    write_outcome::single,
    write_retry::{WRITE_ATTEMPTS, backoff_after_lost_attempts},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    change_notifier::ChangeNotifier,
    entity_snapshot::EntitySnapshot,
    notification_plan::{creation, modification},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::expanded::{
    attribute_system_time::stamp_attribute_instants,
    current_state_fold::{CurrentStateFold, CurrentStateFoldError, fold_into_current_state},
    dataset_id::InstanceDatasetIdError,
};
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;

impl TemporalService {
    /// Add every instance of the expanded `evolution` to the Temporal Evolution
    /// of `entity_id`, and to current state the ones that stand there.
    ///
    /// The fold is computed from current state read outside the writing
    /// transaction, so the write states the version it assumed -- the absence of
    /// the Entity, or its `modifiedAt` -- and is recomputed against a fresh read
    /// when a concurrent writer got in first.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Validation`] when an instance's `datasetId`
    /// cannot be read or the folded Entity cannot be stored,
    /// [`TemporalError::TransactionContended`] when concurrent writers
    /// superseded the fold on every attempt, and [`TemporalError::Commit`] or
    /// [`TemporalError::Persistence`] when the write cannot be landed.
    pub(crate) async fn commit_evolution(&self, target: &ContextTarget, entity_id: &Identifier, evolution: Arc<Value>) -> Result<(), TemporalError> {
        for lost_attempts in 0..WRITE_ATTEMPTS {
            if lost_attempts > 0 {
                self.retry_delay.wait(backoff_after_lost_attempts(lost_attempts)).await;
            }
            let current = self
                .entity_reader
                .get_snapshot(target, entity_id)
                .await
                .map_err(TemporalError::from_entity_failure)?;
            let EvolutionWrite { write, events } = evolution_write(&self.notifier, target, entity_id, Arc::clone(&evolution), current.as_ref())?; // clone: Arc ref-count bump
            let outcome = single(self.writes.commit(target, vec![write]).await?)?;
            match after_attempt(&outcome) {
                AfterAttempt::Settled => {
                    // The events describe the attempt the store accepted, never
                    // one that was computed and lost to a concurrent writer.
                    for event in events {
                        self.notifier.publish(target, event);
                    }
                    return Ok(());
                }
                // A concurrent writer created or changed the Entity, or deleted
                // it, since the read: the fold is recomputed against what it did.
                AfterAttempt::Reread | AfterAttempt::Vanished => (),
            }
        }
        Err(TemporalError::TransactionContended)
    }
}

/// The write that records `evolution` and applies its fold to `current`, with
/// the notifications the fold's change to current state owes.
///
/// Clause 5.8.6 defines a change on current state alone, so the change is
/// classified exactly as a Core write's: a fold that brings the Entity into
/// being is its creation, one that advances it is a modification carrying the
/// instances it replaced, added or removed, and a backfill that leaves current
/// state as it was changes nothing a subscriber can see.
fn evolution_write(
    notifier: &ChangeNotifier,
    target: &ContextTarget,
    entity_id: &Identifier,
    evolution: Arc<Value>,
    current: Option<&EntitySnapshot>,
) -> Result<EvolutionWrite, TemporalError> {
    let now = Utc::now();
    let fold = fold_into_current_state(current.map(|snapshot| &snapshot.payload), &evolution, now).map_err(|error| match error {
        CurrentStateFoldError::DatasetId(InstanceDatasetIdError { source }) => TemporalError::from(SubmissionRefusal::DatasetId(source)),
        CurrentStateFoldError::NotAnEntity => TemporalError::Validation(TemporalValidationReason::EntityNotObject),
    })?;
    let builder = AtomicWrite::builder()
        .entity_id(Arc::new(entity_id.clone())) // clone: the write owns a shared validated identifier.
        .history(vec![HistoryRecord::EvolutionInstances(evolution)]);
    match (current, fold) {
        (Some(snapshot), None) => Ok(EvolutionWrite {
            write: builder
                .entity(EntityMutation::None)
                .projection(CurrentStateProjection::Untouched)
                .precondition(Precondition::UnchangedSince(snapshot.modified_at))
                .build(),
            events: Vec::new(),
        }),
        (current, Some(fold)) => {
            let CurrentStateFold {
                mut document,
                restated,
                removed,
            } = fold;
            // Clause 4.8: a standing instance takes the write instant as its
            // modifiedAt and keeps the createdAt it had; a new one takes both.
            let previous = current.map(|snapshot| &snapshot.payload);
            stamp_attribute_instants(&mut document, previous.unwrap_or(&Value::Null), now).map_err(|error| TemporalError::Internal(error.into()))?;
            let document = Arc::new(document);
            let record = EntityRecord::from_expanded(Arc::clone(&document))?; // clone: Arc ref-count bump; the document is also the notification's
            let change = match previous {
                Some(previous) => modification(previous, &document),
                None => creation(&document),
            }
            .map_err(TemporalError::from_entity_failure)?;
            let plan = notifier.plan(target, entity_id, &record.projection.types, change, Some(&document), previous);
            let projection = CurrentStateProjection::Restated {
                observed: Arc::new(restated),
                removed,
            };
            let (entity, precondition) = match current {
                Some(snapshot) => (EntityMutation::Update(record), Precondition::UnchangedSince(snapshot.modified_at)),
                None => (EntityMutation::Insert(record), Precondition::MustNotExist),
            };
            Ok(EvolutionWrite {
                write: builder
                    .entity(entity)
                    .projection(projection)
                    .subscription_matches(plan.matches)
                    .precondition(precondition)
                    .build(),
                events: plan.events,
            })
        }
        (None, None) => Err(TemporalInvariant::FoldCreatedNoEntity.into()),
    }
}

/// One attempt at committing a Temporal Evolution, and the in-process change
/// events to publish once the store accepted it.
struct EvolutionWrite {
    /// The history, the current-state change and the durable notification
    /// matches, committed together.
    write: AtomicWrite,
    /// The change events matching the durable records, empty when current
    /// state did not change.
    events: Vec<ChangeEvent>,
}

#[cfg(test)]
mod tests {
    use crate::evolution_write::{EvolutionWrite, evolution_write};
    use andromeda_context_commit::entity_mutation::EntityMutation;
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_entity::{change_notifier::ChangeNotifier, entity_snapshot::EntitySnapshot};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{entity_change::EntityLifecycle, expanded::attribute_change::AttributeChangeKind};
    use chrono::Utc;
    use serde_json::{Value, json};
    use std::sync::Arc;

    const SPEED: &str = "https://example.org/speed";
    const HEADING: &str = "https://example.org/heading";
    const OBSERVED_AT: &str = "https://uri.etsi.org/ngsi-ld/observedAt";

    fn entity_id() -> Identifier {
        "urn:ngsi-ld:Vehicle:1".parse().expect("a valid entity identifier")
    }

    /// One expanded Property instance of `value`, observed at `observed_at`.
    fn instance(value: i64, observed_at: &str) -> Value {
        json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}],
            OBSERVED_AT: [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": observed_at}]
        })
    }

    /// An expanded Vehicle carrying `members`.
    fn vehicle(members: &Value) -> Value {
        let mut entity = json!({"@id": "urn:ngsi-ld:Vehicle:1", "@type": ["https://example.org/Vehicle"]});
        for (name, value) in members.as_object().expect("the members are an object") {
            entity[name] = value.clone();
        }
        json!([entity])
    }

    /// Current state holding `speed` = 1, observed on 2024-01-01.
    fn standing() -> EntitySnapshot {
        EntitySnapshot {
            payload: vehicle(&json!({SPEED: [instance(1, "2024-01-01T00:00:00Z")]})),
            modified_at: Utc::now(),
        }
    }

    /// Fold `evolution` into `current` through a notifier that records nothing
    /// durably, as a live-delivery broker does.
    fn written(evolution: &Value, current: Option<&EntitySnapshot>) -> EvolutionWrite {
        evolution_write(
            &ChangeNotifier::new(None, None),
            &ContextTarget::Live,
            &entity_id(),
            Arc::new(evolution.clone()),
            current,
        )
        .expect("the evolution folds")
    }

    /// The lifecycle and the per-instance kinds of the one change a write owes.
    fn owed(write: &EvolutionWrite) -> (EntityLifecycle, Vec<(&str, AttributeChangeKind)>) {
        pretty_assertions::assert_eq!(write.events.len(), 1, "one write owes one change");
        let event = &write.events[0];
        let kinds = event.change.instances.iter().map(|change| (change.key.attr.as_str(), change.kind)).collect();
        (event.change.lifecycle, kinds)
    }

    /// Clause 5.6.11.4 on an unknown identifier brings the Entity into being,
    /// which a subscriber sees as its creation, as Core Create Entity.
    #[test]
    fn a_temporal_create_of_an_unknown_entity_is_notified_as_its_creation() {
        let evolution = vehicle(&json!({SPEED: [instance(1, "2024-01-01T00:00:00Z"), instance(2, "2024-02-01T00:00:00Z")]}));

        let write = written(&evolution, None);

        pretty_assertions::assert_eq!(owed(&write), (EntityLifecycle::Created, vec![(SPEED, AttributeChangeKind::Created)]));
        let event = &write.events[0];
        let notified = event.current_payload.as_deref().expect("the notification renders the created Entity");
        pretty_assertions::assert_eq!(notified[0][SPEED][0]["https://uri.etsi.org/ngsi-ld/hasValue"], json!([{"@value": 2}]));
    }

    /// A newer observation replaces the standing instance (clause 4.5.5.3), an
    /// `attributeUpdated` change of current state, and carries the replaced
    /// instance for `showChanges`.
    #[test]
    fn a_newer_observation_is_notified_as_an_update_of_the_standing_instance() {
        let current = standing();
        let evolution = vehicle(&json!({SPEED: [instance(5, "2024-06-01T00:00:00Z")]}));

        let write = written(&evolution, Some(&current));

        pretty_assertions::assert_eq!(owed(&write), (EntityLifecycle::Modified, vec![(SPEED, AttributeChangeKind::Updated)]));
        let previous = write.events[0].previous_attributes.as_deref().expect("the replaced state is carried");
        pretty_assertions::assert_eq!(previous[0][SPEED][0]["https://uri.etsi.org/ngsi-ld/hasValue"], json!([{"@value": 1}]));
    }

    /// An attribute current state did not hold is created beside the standing
    /// ones, which stay out of the change.
    #[test]
    fn a_new_attribute_is_notified_as_created_and_the_untouched_one_is_not() {
        let current = standing();
        let evolution = vehicle(&json!({HEADING: [instance(90, "2024-06-01T00:00:00Z")]}));

        let write = written(&evolution, Some(&current));

        pretty_assertions::assert_eq!(owed(&write), (EntityLifecycle::Modified, vec![(HEADING, AttributeChangeKind::Created)]));
    }

    /// A backfill of older observations leaves current state as it was, so it
    /// is no change under clause 5.8.6 and owes no notification.
    #[test]
    fn a_backfill_leaves_current_state_alone_and_owes_no_notification() {
        let current = standing();
        let evolution = vehicle(&json!({SPEED: [instance(0, "2023-01-01T00:00:00Z")]}));

        let write = written(&evolution, Some(&current));

        assert!(matches!(write.write.entity, EntityMutation::None));
        assert!(write.write.subscription_matches.is_empty());
        assert!(write.events.is_empty());
    }

    /// Clause 5.8.6: "An Attribute is considered to change when any of the
    /// members [...] is updated with a value different than the existing
    /// one". Re-sending the standing observation replaces it with itself.
    #[test]
    fn restating_the_standing_observation_owes_no_notification() {
        let current = standing();
        let evolution = vehicle(&json!({SPEED: [instance(1, "2024-01-01T00:00:00Z")]}));

        let write = written(&evolution, Some(&current));

        assert!(write.events.is_empty());
    }

    /// An instance recording a deletion (clause 4.5.7) that stands over the
    /// current one removes it from current state, an `attributeDeleted` change.
    #[test]
    fn a_standing_deletion_is_notified_as_the_removal_of_the_instance() {
        let current = standing();
        let mut deletion = instance(1, "2024-06-01T00:00:00Z");
        deletion["https://uri.etsi.org/ngsi-ld/deletedAt"] = json!([{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2024-06-01T00:00:00Z"}]);
        let evolution = vehicle(&json!({SPEED: [deletion]}));

        let write = written(&evolution, Some(&current));

        pretty_assertions::assert_eq!(owed(&write), (EntityLifecycle::Modified, vec![(SPEED, AttributeChangeKind::Deleted)]));
    }
}
