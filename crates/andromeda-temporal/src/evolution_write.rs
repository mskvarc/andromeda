//! Committing instances added to a Temporal Evolution together with the
//! current-state change they make.
//!
//! Clauses 5.6.11.4 and 5.6.12.4 add every instance "to the target Temporal
//! Evolution of an Entity", so all of them are history. Current state holds
//! one instance per Attribute instance (clause 4.5.5), the most current one
//! (clause 4.5.5.3), so it takes only the instances that stand over what it
//! held: a Temporal Evolution created for an unknown identifier brings the
//! Entity into being with its most current instances, a newer observation
//! advances it, and a backfill of older ones leaves it as it was. So does the
//! end of an instance current state does not hold, or an instance restating a
//! held one: the Entity row is not rewritten and keeps its `modifiedAt`
//! (clause 4.8). Current state changes in the transaction that records the
//! history.
//!
//! Subscriptions watch current state (clause 5.8.6 defines a change on it
//! alone), so a change the fold makes owes the notifications a Core write
//! making the same change would, committed with it; a backfill owes none.

use crate::{error::TemporalError, service::TemporalService, temporal_invariant::TemporalInvariant, written_evolution::WrittenEvolution};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    attempt_instant::AttemptInstant,
    change_event::ChangeEvent,
    entity_mutation::EntityMutation,
    entity_record::EntityRecord,
    precondition::{AfterAttempt, Precondition, after_attempt},
    write_outcome::single,
    write_retry::WRITE_RETRY,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    change_notifier::ChangeNotifier,
    entity_snapshot::EntitySnapshot,
    notification_plan::{WrittenEntity, created_sys_attrs, creation, modification, modified_sys_attrs},
    stored_census::admit_stored_entity,
};
use andromeda_ngsi_ld_entity::history::current_state_fold::fold_into_current_state;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use derive_more::{From, IsVariant};
use std::sync::Arc;

impl TemporalService {
    /// Add every instance of `evolution` to the Temporal Evolution of
    /// `entity_id`, and to current state the ones that stand there.
    ///
    /// The fold is computed from current state read outside the writing
    /// transaction, so the write states the version it assumed -- the absence of
    /// the Entity, or its `modifiedAt` -- and is recomputed against a fresh read
    /// when a concurrent writer got in first. An attempt the store refuses for
    /// being stamped before an instant the Entity holds is stamped again after
    /// it (clause 4.8).
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Validation`] when the evolution would create an
    /// Entity naming no identifier or type, or a standing instance cannot be
    /// current state, [`TemporalError::TransactionContended`] when concurrent
    /// writers superseded the fold on every attempt, and
    /// [`TemporalError::Commit`] or [`TemporalError::Persistence`] when the
    /// write cannot be landed.
    pub(crate) async fn commit_evolution(&self, target: &ContextTarget, entity_id: &Identifier, evolution: &WrittenEvolution) -> Result<(), TemporalError> {
        let mut instant = AttemptInstant::default();
        let settled = WRITE_RETRY
            .run(
                &*self.retry_delay,
                |attempt: &EvolutionAttempt, _| attempt.is_lost(),
                async |_| {
                    let current = self
                        .entity_reader
                        .get_snapshot(target, entity_id)
                        .await
                        .map_err(TemporalError::from_entity_failure)?;
                    let now = instant.next();
                    let EvolutionWrite { write, events } = evolution_write(
                        &self.notifier,
                        target,
                        entity_id,
                        evolution,
                        current.as_ref(),
                        now,
                        self.limits.payload_budget.json_values,
                    )?;
                    let committed = self.writes.commit(target, now, vec![write]).await.map_err(TemporalError::from)?;
                    let outcome = single(committed).map_err(TemporalError::from)?;
                    match after_attempt(&outcome) {
                        AfterAttempt::Settled => Ok((events, outcome.changed_at())),
                        // A concurrent writer created or changed the Entity, or
                        // deleted it, since the read: the fold is recomputed
                        // against what it did.
                        AfterAttempt::Reread | AfterAttempt::Vanished => Err(EvolutionAttempt::Lost),
                        AfterAttempt::Restamp(latest) => {
                            instant.refused(latest);
                            Err(EvolutionAttempt::Lost)
                        }
                    }
                },
            )
            .await;
        match settled {
            Ok((events, committed_at)) => {
                // The events describe the attempt the store accepted, never one
                // that was computed and lost to a concurrent writer. A write that
                // left current state as it was changed no entity, and owes no
                // notification.
                if let Some(committed_at) = committed_at {
                    self.notifier.publish(target, committed_at, events).await;
                }
                Ok(())
            }
            Err(EvolutionAttempt::Lost) => Err(TemporalError::TransactionContended),
            Err(EvolutionAttempt::Failed(error)) => Err(error),
        }
    }
}

/// Why one attempt of an evolution write did not settle.
#[derive(Debug, From, IsVariant)]
enum EvolutionAttempt {
    /// A concurrent writer superseded the current state the fold was computed
    /// from.
    #[from(ignore)]
    Lost,
    /// The attempt failed for a reason another attempt cannot clear.
    Failed(TemporalError),
}

/// The write that records `evolution` and applies its fold to `current`, with
/// the notifications the fold's change to current state owes.
///
/// Clause 5.8.6 defines a change on current state alone, so the change is
/// classified exactly as a Core write's: a fold that brings the Entity into
/// being is its creation, one that advances it is a modification carrying the
/// instances it replaced, added or removed, and a fold that leaves current
/// state as it was (a backfill, the end of an instance it does not hold, a
/// restatement) changes nothing a subscriber can see and leaves the Entity
/// row as it was, the version it read standing as the precondition. The fold
/// settles at `now`, the instant the commit records the evolution at.
///
/// The evolution document is counted against the payload budget on its own
/// when it is admitted; the current state the fold stores is held to `limit`
/// as a Core write's is, so evolutions each within the budget cannot grow the
/// Entity past it.
fn evolution_write(
    notifier: &ChangeNotifier,
    target: &ContextTarget,
    entity_id: &Identifier,
    evolution: &WrittenEvolution,
    current: Option<&EntitySnapshot>,
    now: SystemInstant,
    limit: JsonValueLimit,
) -> Result<EvolutionWrite, TemporalError> {
    let previous = current.map(|snapshot| &snapshot.state);
    let fold = fold_into_current_state(previous.map(|state| &**state), evolution.document(), now.get())?;
    let builder = AtomicWrite::builder()
        .entity_id(Arc::new(entity_id.clone())) // clone: the write owns a shared validated identifier.
        .history(evolution.history());
    match (current, fold) {
        (Some(snapshot), None) => Ok(EvolutionWrite {
            write: builder
                .entity(EntityMutation::None)
                .precondition(Precondition::UnchangedSince(snapshot.version))
                .build(),
            events: Vec::new(),
        }),
        (current, Some(mut state)) => {
            // Clause 4.8: a standing instance takes the write instant as its
            // modifiedAt and keeps the createdAt it had; a new one takes both.
            state.settle(previous.map(|state| &**state), now);
            admit_stored_entity(limit, previous.map(|state| &**state), &state)?;
            let state = Arc::new(state);
            // clone: Arc ref-count bump; the state is also the notification's
            let record = match previous {
                Some(previous) => EntityRecord::replacing(previous, Arc::clone(&state))?,
                None => EntityRecord::from_state(Arc::clone(&state))?,
            };
            let (change, sys_attrs) = match current {
                Some(snapshot) => (modification(&snapshot.state, &state), modified_sys_attrs(&snapshot.sys_attrs, &state, now)),
                None => (creation(&state), created_sys_attrs(&state, now)),
            };
            let written = WrittenEntity {
                state: &state,
                sys_attrs: &sys_attrs,
            };
            let plan = notifier.plan(target, entity_id, &record.projection.types, change, written, previous);
            let (entity, precondition) = match current {
                Some(snapshot) => (EntityMutation::Update(record), Precondition::UnchangedSince(snapshot.version)),
                None => (EntityMutation::Insert(record), Precondition::MustNotExist),
            };
            Ok(EvolutionWrite {
                write: builder.entity(entity).subscription_matches(plan.matches).precondition(precondition).build(),
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
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        error::TemporalError,
        evolution_write::{EvolutionWrite, evolution_write},
        written_evolution::WrittenEvolution,
    };
    use andromeda_context_commit::{entity_mutation::EntityMutation, entity_version::EntityVersion, precondition::Precondition};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_entity::{
        change_notifier::{ChangeNotifier, SubscriptionMatching},
        entity_snapshot::EntitySnapshot,
        stored_census::stored_value_count,
    };
    use andromeda_ngsi_ld_entity::{
        document::EntityDocument,
        entity_state::EntityState,
        expanded_value::ExpandedValue,
        family_value::FamilyValue,
        literal::LiteralValue,
        reads::attribute_change::AttributeChangeKind,
    };
    use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier, time::system_instant::SystemInstant};
    use andromeda_ngsi_ld_model::{entity_change::EntityLifecycle, notification::SysAttrsSnapshot};
    use andromeda_ngsi_ld_number::{JsonNumber, json_value::value_census::JsonValueLimit};
    use andromeda_ngsi_ld_representation::payload_budget::PayloadBudgetExceeded;
    use chrono::{TimeZone, Utc};
    use serde_json::{Value, json};
    use std::{num::NonZeroUsize, sync::Arc};

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
        if let (Some(entity), Some(members)) = (entity.as_object_mut(), members.as_object()) {
            entity.extend(members.iter().map(|(name, value)| (name.clone(), value.clone()))); // clone: the fixture owns its members
        }
        json!([entity])
    }

    /// Current state holding `speed` = 1, observed on 2024-01-01.
    fn standing() -> EntitySnapshot {
        let stored = vehicle(&json!({SPEED: [instance(1, "2024-01-01T00:00:00Z")]}));
        EntitySnapshot {
            state: Arc::new(EntityState::from_slice(&serde_json::to_vec(&stored).expect("JSON")).expect("a stored entity decodes")),
            version: EntityVersion::new(1, 1),
            sys_attrs: SysAttrsSnapshot {
                created_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).single().expect("a valid instant"),
                modified_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).single().expect("a valid instant"),
                expires_at: None,
            },
        }
    }

    /// Fold `evolution` into `current` through a notifier that records nothing
    /// durably, as a live-delivery broker does.
    fn written(evolution: &Value, current: Option<&EntitySnapshot>) -> EvolutionWrite {
        written_at(evolution, current, SystemInstant::new(Utc::now()))
    }

    /// [`written`], settled at `now`.
    fn written_at(evolution: &Value, current: Option<&EntitySnapshot>, now: SystemInstant) -> EvolutionWrite {
        folded(evolution, current, now, limit(usize::MAX)).expect("the evolution folds")
    }

    /// Fold `evolution` into `current` at `now`, holding current state to
    /// `limit`.
    fn folded(evolution: &Value, current: Option<&EntitySnapshot>, now: SystemInstant, limit: JsonValueLimit) -> Result<EvolutionWrite, TemporalError> {
        let document = EntityDocument::from_slice(&serde_json::to_vec(evolution).expect("JSON")).expect("a written evolution decodes");
        evolution_write(
            &ChangeNotifier::new(None, SubscriptionMatching::Absent),
            &ContextTarget::Live,
            &entity_id(),
            &WrittenEvolution::new(Arc::new(document), None),
            current,
            now,
            limit,
        )
    }

    fn limit(values: usize) -> JsonValueLimit {
        JsonValueLimit::new(NonZeroUsize::new(values).expect("a positive value limit"))
    }

    /// The lifecycle and the per-instance kinds of the one change a write owes.
    fn owed(write: &EvolutionWrite) -> (EntityLifecycle, Vec<(&str, AttributeChangeKind)>) {
        pretty_assertions::assert_eq!(write.events.len(), 1, "one write owes one change");
        let event = &write.events[0];
        let kinds = event.change.instances.iter().map(|change| (change.key.attr.as_str(), change.kind)).collect();
        (event.change.lifecycle, kinds)
    }

    /// The value the default `speed` instance of `state` holds.
    fn speed(state: &EntityState) -> Option<JsonNumber> {
        let (_, instance) = state.attributes().get(SPEED)?.default_instance()?;
        let FamilyValue::Property { values, .. } = instance.value() else {
            return None;
        };
        match values.as_slice() {
            [ExpandedValue::Literal(literal)] => match literal.value() {
                LiteralValue::Number(number) => Some(number.clone()), // clone: the assertion owns the number
                LiteralValue::String(_) | LiteralValue::Boolean(_) => None,
            },
            [ExpandedValue::Json(_) | ExpandedValue::Node(_) | ExpandedValue::List(_)] | [] | [_, _, ..] => None,
        }
    }

    /// Clause 5.6.11.4 on an unknown identifier brings the Entity into being,
    /// which a subscriber sees as its creation, as Core Create Entity.
    #[test]
    fn a_temporal_create_of_an_unknown_entity_is_notified_as_its_creation() {
        let evolution = vehicle(&json!({SPEED: [instance(1, "2024-01-01T00:00:00Z"), instance(2, "2024-02-01T00:00:00Z")]}));

        let write = written(&evolution, None);

        pretty_assertions::assert_eq!(owed(&write), (EntityLifecycle::Created, vec![(SPEED, AttributeChangeKind::Created)]));
        let notified = write.events[0].current_payload.as_deref().expect("the notification renders the created Entity");
        pretty_assertions::assert_eq!(speed(notified), Some(JsonNumber::from(2)));
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
        pretty_assertions::assert_eq!(speed(previous), Some(JsonNumber::from(1)));
    }

    /// Clause 4.8: the instance the fold makes current takes the instant the
    /// commit records the evolution at as its `modifiedAt`.
    #[test]
    fn the_fold_settles_at_the_instant_the_commit_records_the_evolution_at() {
        let current = standing();
        let now = SystemInstant::new(Utc.with_ymd_and_hms(2026, 10, 23, 8, 0, 0).single().expect("a valid instant"));
        let evolution = vehicle(&json!({SPEED: [instance(5, "2024-06-01T00:00:00Z")]}));

        let write = written_at(&evolution, Some(&current), now);

        let state = &write.write.entity.record().expect("the fold stores current state").state;
        let (_, instance) = state
            .attributes()
            .get(SPEED)
            .and_then(|attribute| attribute.default_instance())
            .expect("a speed instance");
        pretty_assertions::assert_eq!(instance.modified_at(), Some(now));
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

    /// Entity Type names carry no instant, so a backfill naming a type the
    /// Entity lacks still adds it to current state, the types its Temporal
    /// Evolution is then recorded under. No attribute changed, so clause 5.8.6
    /// sees no change and no notification is owed.
    #[test]
    fn a_backfill_naming_a_new_type_adds_it_to_current_state_and_owes_no_notification() {
        let current = standing();
        let mut evolution = vehicle(&json!({SPEED: [instance(0, "2023-01-01T00:00:00Z")]}));
        evolution[0]["@type"] = json!(["https://example.org/Car"]);

        let write = written(&evolution, Some(&current));

        let EntityMutation::Update(record) = &write.write.entity else {
            panic!("the new type updates current state: {:?}", write.write.entity);
        };
        let types: Vec<&str> = record.projection.types.iter().map(EntityTypeName::as_str).collect();
        pretty_assertions::assert_eq!(types, ["https://example.org/Vehicle", "https://example.org/Car"]);
        pretty_assertions::assert_eq!(speed(&record.state), Some(JsonNumber::from(1)));
        assert!(write.write.subscription_matches.is_empty());
        assert!(write.events.is_empty());
    }

    /// Clause 5.8.6: "An Attribute is considered to change when any of the
    /// members [...] is updated with a value different than the existing
    /// one". Re-sending the standing observation modifies nothing, so clause
    /// 4.8 leaves the Entity's `modifiedAt` where it was.
    #[test]
    fn restating_the_standing_observation_leaves_the_entity_row_alone_and_owes_no_notification() {
        let current = standing();
        let evolution = vehicle(&json!({SPEED: [instance(1, "2024-01-01T00:00:00Z")]}));

        let write = written(&evolution, Some(&current));

        assert!(matches!(write.write.entity, EntityMutation::None), "{:?}", write.write.entity);
        assert!(matches!(write.write.precondition, Precondition::UnchangedSince(version) if version == current.version));
        assert!(write.write.subscription_matches.is_empty());
        assert!(write.events.is_empty());
    }

    /// Clause 4.5.7 records the deletion in the Temporal Evolution, but
    /// current state holds no instance for it to end, so the Entity row is
    /// left alone while the history is still written.
    #[test]
    fn the_end_of_an_instance_current_state_does_not_hold_leaves_the_entity_row_alone() {
        let current = standing();
        let mut deletion = instance(1, "2024-06-01T00:00:00Z");
        deletion["https://uri.etsi.org/ngsi-ld/deletedAt"] = json!([{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2024-06-01T00:00:00Z"}]);
        let evolution = vehicle(&json!({HEADING: [deletion]}));

        let write = written(&evolution, Some(&current));

        assert!(matches!(write.write.entity, EntityMutation::None), "{:?}", write.write.entity);
        assert!(matches!(write.write.precondition, Precondition::UnchangedSince(version) if version == current.version));
        assert!(!write.write.history.is_empty(), "the deletion is still recorded in the Temporal Evolution");
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

    /// Each evolution fits the budget on its own, but the attribute it adds
    /// would take current state past the entity value limit, so the fold is
    /// refused rather than stored.
    #[test]
    fn a_fold_growing_current_state_past_the_value_limit_is_refused() {
        let current = standing();
        let held = stored_value_count(&current.state).get();
        let evolution = vehicle(&json!({HEADING: [instance(90, "2024-06-01T00:00:00Z")]}));
        let now = SystemInstant::new(Utc::now());

        let refusal = folded(&evolution, Some(&current), now, limit(held)).err();

        assert!(
            matches!(refusal, Some(TemporalError::PayloadBudget(PayloadBudgetExceeded::EntityJsonValues { .. }))),
            "{refusal:?}"
        );
        folded(&evolution, Some(&current), now, limit(2 * held)).expect("the grown entity fits twice the standing count");
    }

    /// A newer observation of the standing attribute keeps current state at
    /// its size, so it lands even when the limit was lowered below it.
    #[test]
    fn a_fold_keeping_current_state_at_its_size_lands_under_a_lowered_limit() {
        let current = standing();
        let evolution = vehicle(&json!({SPEED: [instance(5, "2024-06-01T00:00:00Z")]}));

        let write = folded(&evolution, Some(&current), SystemInstant::new(Utc::now()), limit(1)).expect("the fold does not grow the entity");

        assert!(matches!(write.write.entity, EntityMutation::Update(_)), "{:?}", write.write.entity);
    }
}
