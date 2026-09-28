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

use crate::{error::TemporalError, service::TemporalService, temporal_validation_reason::TemporalValidationReason};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    current_state_projection::CurrentStateProjection,
    entity_mutation::EntityMutation,
    entity_record::EntityRecord,
    history_record::HistoryRecord,
    precondition::{AfterAttempt, Precondition, after_attempt},
    write_outcome::single,
    write_retry::{WRITE_ATTEMPTS, backoff_after_lost_attempts},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::entity_snapshot::EntitySnapshot;
use andromeda_ngsi_ld_lexicon::uri::Uri;
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
    pub(crate) async fn commit_evolution(&self, target: &ContextTarget, entity_id: &Uri, evolution: Arc<Value>) -> Result<(), TemporalError> {
        for lost_attempts in 0..WRITE_ATTEMPTS {
            if lost_attempts > 0 {
                self.retry_delay.wait(backoff_after_lost_attempts(lost_attempts)).await;
            }
            let current = self
                .entity_reader
                .get_snapshot(target, entity_id)
                .await
                .map_err(|error| TemporalError::persistence(error.to_string()))?;
            let write = evolution_write(entity_id, Arc::clone(&evolution), current.as_ref())?; // clone: Arc ref-count bump
            let outcome = single(self.writes.commit(target, vec![write]).await?)?;
            match after_attempt(&outcome) {
                AfterAttempt::Settled => return Ok(()),
                // A concurrent writer created or changed the Entity, or deleted
                // it, since the read: the fold is recomputed against what it did.
                AfterAttempt::Reread | AfterAttempt::Vanished => (),
            }
        }
        Err(TemporalError::TransactionContended)
    }
}

/// The write that records `evolution` and applies its fold to `current`.
fn evolution_write(entity_id: &Uri, evolution: Arc<Value>, current: Option<&EntitySnapshot>) -> Result<AtomicWrite, TemporalError> {
    let now = Utc::now();
    let fold = fold_into_current_state(current.map(|snapshot| &snapshot.payload), &evolution, now).map_err(|error| match error {
        CurrentStateFoldError::DatasetId(InstanceDatasetIdError { raw, source }) => {
            TemporalError::Validation(TemporalValidationReason::DatasetId { raw, source })
        }
        CurrentStateFoldError::NotAnEntity => TemporalError::Validation(TemporalValidationReason::EntityNotObject),
    })?;
    let builder = AtomicWrite::builder()
        .entity_id(Arc::new(entity_id.clone())) // clone: the write owns a shared validated identifier.
        .history(vec![HistoryRecord::EvolutionInstances(evolution)]);
    let write = match (current, fold) {
        (Some(snapshot), None) => builder
            .entity(EntityMutation::None)
            .projection(CurrentStateProjection::Untouched)
            .precondition(Precondition::UnchangedSince(snapshot.modified_at))
            .build(),
        (current, Some(fold)) => {
            let CurrentStateFold {
                mut document,
                restated,
                removed,
            } = fold;
            // Clause 4.8: a standing instance takes the write instant as its
            // modifiedAt and keeps the createdAt it had; a new one takes both.
            let previous = current.map_or(&Value::Null, |snapshot| &snapshot.payload);
            stamp_attribute_instants(&mut document, previous, now).map_err(TemporalError::internal)?;
            let record = EntityRecord::from_expanded(Arc::new(document))?;
            let projection = CurrentStateProjection::Restated {
                observed: Arc::new(restated),
                removed,
            };
            match current {
                Some(snapshot) => builder
                    .entity(EntityMutation::Update(record))
                    .projection(projection)
                    .precondition(Precondition::UnchangedSince(snapshot.modified_at))
                    .build(),
                None => builder
                    .entity(EntityMutation::Insert(record))
                    .projection(projection)
                    .precondition(Precondition::MustNotExist)
                    .build(),
            }
        }
        (None, None) => return Err(TemporalError::internal("folding a Temporal Evolution into no Entity created none")),
    };
    Ok(write)
}
