//! Create Entity (clause 5.6.1).
//!
//! A create reads nothing: it holds only while no Entity has a current state
//! under its identifier. An identifier whose identity row holds a Temporal
//! Evolution alone is created anew on that row, and the store refuses the
//! creation when the row holds an instant later than the create's, a
//! deletion's or a Temporal API write's: its instances would be recorded
//! before them (clause 4.8). The create is then stamped again after that
//! instant.

use crate::{
    EntityService,
    admitted_write::AdmittedEntity,
    contended_attempt::ContendedAttempt,
    error::EntityError,
    notification_plan::{NotificationPlan, WrittenEntity, created_sys_attrs, creation},
    write_kind::CreateEntity,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    attempt_instant::AttemptInstant,
    change_event::ChangeEvent,
    entity_mutation::EntityMutation,
    entity_record::EntityRecord,
    history_record::HistoryRecord,
    precondition::Precondition,
    write_outcome::{WriteOutcome, single},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use andromeda_telemetry::entity::{self, EntityOperation};
use std::sync::Arc;

impl EntityService {
    /// Create the new NGSI-LD entity `id` from its admitted payload, returning
    /// its URI on success (clause 5.6.1).
    ///
    /// Clause 5.6.1.4 opens with the clause 5.5.4 validation, which produced
    /// `admitted` before the operation forwarded anything.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Validation`] when the written document is no
    /// entity a creation can store (clause 5.6.1.4), [`EntityError::AlreadyExists`] when another entity already holds
    /// the identifier (clause 6.4.3.1), and [`EntityError::Persistence`] when
    /// the store could not commit the insert.
    pub async fn create(&self, target: &ContextTarget, id: &Identifier, admitted: AdmittedEntity<CreateEntity>) -> Result<Identifier, EntityError> {
        let timer = entity::start(EntityOperation::Create);
        let result: Result<Identifier, EntityError> = async {
            let mut created = Arc::new(EntityState::try_from(admitted.into_document())?);
            let mut instant = AttemptInstant::default();
            let (outcome, events) = self
                .retry_contended(async |_| {
                    let now = instant.next();
                    // Clause 4.8: a created entity's attribute instances carry the
                    // write instant as both createdAt and modifiedAt. Only this
                    // attempt holds the state once a refused write is dropped, so
                    // stamping it again copies nothing.
                    Arc::make_mut(&mut created).settle(None, now);
                    let (write, events) = self.creation_write(target, id, &created, now)?;
                    let outcome = single(self.writes.commit(target, now, vec![write]).await.map_err(EntityError::from)?).map_err(EntityError::from)?;
                    match outcome {
                        WriteOutcome::Predated(latest) => {
                            instant.refused(latest);
                            Err(ContendedAttempt::Lost)
                        }
                        landed @ (WriteOutcome::Created(_)
                        | WriteOutcome::Updated(_)
                        | WriteOutcome::Deleted(..)
                        | WriteOutcome::AlreadyExists
                        | WriteOutcome::NotFound
                        | WriteOutcome::Superseded
                        | WriteOutcome::Skipped
                        | WriteOutcome::NoEntityChange
                        | WriteOutcome::Refused(_)) => Ok((landed, events)),
                    }
                })
                .await
                .map_err(|failure| failure.into_entity_error(id))?;

            // A conflicting insert is 409, per NGSI-LD 6.4.3.1. It reaches here as a
            // precondition failure rather than a duplicate-key error, which is what
            // lets the same statement serve a partially-succeeding batch.
            if outcome.is_already_exists() {
                return Err(EntityError::already_exists(id));
            }

            if let Some(committed_at) = outcome.changed_at() {
                self.notifier.publish(target, committed_at, events).await;
            }

            Ok(id.clone()) // clone: the caller owns the created identifier while the request entity remains borrowed
        }
        .await;
        timer.finish_result(&result);
        result
    }

    /// The write that stores `created`, settled at `now`, as the new entity
    /// `id`, and the change events it owes once it lands: one `entityCreated`
    /// over the whole entity, and the whole entity as its first observation.
    ///
    /// # Errors
    ///
    /// Reports the failure to derive the entity's stored columns.
    fn creation_write(
        &self,
        target: &ContextTarget,
        id: &Identifier,
        created: &Arc<EntityState>,
        now: SystemInstant,
    ) -> Result<(AtomicWrite, Vec<ChangeEvent>), EntityError> {
        let sys_attrs = created_sys_attrs(created, now);
        let written = WrittenEntity {
            state: created,
            sys_attrs: &sys_attrs,
        };
        let NotificationPlan { matches, events } = self.notifier.plan(target, id, created.types().as_slice(), creation(created), written, None);
        let write = AtomicWrite::builder()
            .entity_id(Arc::new(id.clone())) // clone: the write owns a shared validated identifier.
            .entity(EntityMutation::Insert(EntityRecord::from_state(Arc::clone(created))?)) // clone: Arc ref-count bump; the state is also the change event's
            .history(vec![HistoryRecord::Observations(Arc::clone(created))]) // clone: Arc ref-count bump; the stored state is also the write's observation
            .subscription_matches(matches)
            .precondition(Precondition::MustNotExist)
            .build();
        Ok((write, events))
    }
}
