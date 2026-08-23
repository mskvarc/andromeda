//! Applying clause 4.22 expiry through the ordinary atomic mutation boundary.

use crate::{EntityService, entity_snapshot::EntitySnapshot, error::EntityError, notification_plan::attribute_change_groups, port::expiry::EntityExpirySource};
#[cfg(test)]
use andromeda_context_commit::write_outcome::WriteOutcome;
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    entity_mutation::EntityMutation,
    entity_record::EntityRecord,
    history_record::HistoryRecord,
    precondition::Precondition,
};
use andromeda_context_space::context_target::ContextTarget;
#[cfg(test)]
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::expanded::{
    entity_member::extract_types,
    mutation::expiry::remove_expired_attribute_instances,
    removed_instance::removed_attribute_instances,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{num::NonZeroU32, sync::Arc};
use thiserror::Error;
use tracing::warn;

/// One entity-policy-owned bounded expiry operation.
pub struct EntityExpirySweep {
    target: ContextTarget,
    entity_policy: Arc<EntityService>,
    source: Arc<dyn EntityExpirySource>,
    batch_size: EntityExpiryBatchSize,
}

impl EntityExpirySweep {
    /// Construct expiry policy over its semantic service and candidate source.
    pub const fn new(
        target: ContextTarget,
        entity_policy: Arc<EntityService>,
        source: Arc<dyn EntityExpirySource>,
        batch_size: EntityExpiryBatchSize,
    ) -> EntityExpirySweep {
        EntityExpirySweep {
            target,
            entity_policy,
            source,
            batch_size,
        }
    }

    /// Reconcile entity and attribute-instance expiry at one explicit UTC cutoff.
    pub async fn execute(&self, cutoff: DateTime<Utc>) -> Result<EntityExpiryOutcome, EntityError> {
        if self.entity_policy.batch_activity.is_within_cooldown() {
            return Ok(EntityExpiryOutcome::DeferredForBatchActivity);
        }
        let expired_entities = self
            .entity_policy
            .expire_due_entities(&self.target, self.source.as_ref(), cutoff, self.batch_size)
            .await?;
        let purged_attributes = self
            .entity_policy
            .purge_expired_attributes(&self.target, self.source.as_ref(), cutoff, self.batch_size)
            .await?;
        Ok(EntityExpiryOutcome::Applied {
            expired_entities,
            purged_attributes,
        })
    }
}

/// Non-zero maximum candidate count read by each phase of one expiry sweep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityExpiryBatchSize(NonZeroU32);

impl EntityExpiryBatchSize {
    /// Return the validated candidate limit.
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

impl TryFrom<usize> for EntityExpiryBatchSize {
    type Error = EntityExpiryBatchSizeError;

    fn try_from(value: usize) -> Result<EntityExpiryBatchSize, EntityExpiryBatchSizeError> {
        let value = u32::try_from(value).map_err(|_| EntityExpiryBatchSizeError::TooLarge)?;
        let value = NonZeroU32::new(value).ok_or(EntityExpiryBatchSizeError::Zero)?;
        Ok(EntityExpiryBatchSize(value))
    }
}

/// Invalid expiry sweep candidate limit.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum EntityExpiryBatchSizeError {
    /// A bounded sweep cannot have an empty limit.
    #[error("entity expiry batch size must be at least one")]
    Zero,
    /// The configured limit exceeds the portable 32-bit port representation.
    #[error("entity expiry batch size must fit in an unsigned 32-bit integer")]
    TooLarge,
}

/// Semantic result of one bounded entity expiry reconciliation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityExpiryOutcome {
    /// Recent batch activity deferred this level-triggered reconciliation.
    DeferredForBatchActivity,
    /// Expiry reconciliation completed against the supplied cutoff.
    Applied {
        /// Entity-level deletions that committed.
        expired_entities: usize,
        /// Entity payloads whose expired attribute instances were removed.
        purged_attributes: usize,
    },
}

impl EntityService {
    /// Delete every entity-level expiry candidate through normal entity deletion policy.
    async fn expire_due_entities(
        &self,
        target: &ContextTarget,
        source: &dyn EntityExpirySource,
        cutoff: DateTime<Utc>,
        batch_size: EntityExpiryBatchSize,
    ) -> Result<usize, EntityError> {
        let candidates = source.entities_due_for_expiry(target, cutoff, batch_size).await?;
        let mut expired = 0usize;
        for entity_id in candidates {
            match self.delete(target, &entity_id).await {
                Ok(()) => expired += 1,
                Err(EntityError::NotFound { identifier: _ }) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(expired)
    }

    /// Remove every past-due attribute instance and record its history tombstone atomically.
    async fn purge_expired_attributes(
        &self,
        target: &ContextTarget,
        source: &dyn EntityExpirySource,
        cutoff: DateTime<Utc>,
        batch_size: EntityExpiryBatchSize,
    ) -> Result<usize, EntityError> {
        let candidates = source.entities_with_expired_attributes(target, cutoff, batch_size).await?;
        let mut writes = Vec::new();
        let mut pending = Vec::new();
        for (identifier, snapshot) in candidates {
            let Some(purged) = purged_state(snapshot, cutoff) else {
                warn!(entity = %identifier, "attribute-expiry candidate held no past-due instance");
                continue;
            };
            let types = extract_types(&purged.current);
            let groups = attribute_change_groups(&purged.previous, &purged.current);
            let plan = self.plan_notifications(target, &identifier, &types, &groups, Some(&purged.current), Some(&purged.previous));
            writes.push(
                AtomicWrite::builder()
                    .entity_id(Arc::new(identifier.clone()))
                    .entity(EntityMutation::Update(EntityRecord::from_expanded(Arc::clone(&purged.current))))
                    .history(purged.history)
                    .subscription_matches(plan.matches)
                    .precondition(Precondition::UnchangedSince(purged.modified_at))
                    .build(),
            );
            pending.push((identifier, plan.events));
        }
        if writes.is_empty() {
            return Ok(0);
        }

        let outcomes = self.writes.commit(target, writes).await?;
        let mut landed = 0;
        for ((_, events), outcome) in pending.into_iter().zip(&outcomes) {
            if outcome.is_updated() {
                landed += 1;
                events.into_iter().for_each(|event| self.emit_change(target, event));
            }
        }
        Ok(landed)
    }
}

#[cfg(test)]
fn landed_ids(identifiers: Vec<Uri>, outcomes: &[WriteOutcome]) -> Vec<Uri> {
    identifiers
        .into_iter()
        .zip(outcomes)
        .filter(|(_, outcome)| outcome.is_updated())
        .map(|(identifier, _)| identifier)
        .collect()
}

struct PurgedState {
    previous: Value,
    current: Arc<Value>,
    modified_at: DateTime<Utc>,
    history: Vec<HistoryRecord>,
}

fn purged_state(snapshot: EntitySnapshot, cutoff: DateTime<Utc>) -> Option<PurgedState> {
    let EntitySnapshot {
        payload: previous,
        modified_at,
    } = snapshot;
    let mut purged = previous.clone(); // clone: history diffing requires both the before and after payloads.
    if !remove_expired_attribute_instances(&mut purged, cutoff) {
        return None;
    }
    let history = removed_attribute_instances(&previous, &purged)
        .into_iter()
        .map(HistoryRecord::Tombstone)
        .collect();
    Some(PurgedState {
        previous,
        current: Arc::new(purged),
        modified_at,
        history,
    })
}

#[cfg(test)]
fn purge_write(identifier: &Uri, snapshot: EntitySnapshot, cutoff: DateTime<Utc>) -> Option<AtomicWrite> {
    let purged = purged_state(snapshot, cutoff)?;
    Some(
        AtomicWrite::builder()
            .entity_id(Arc::new(identifier.clone())) // clone: the atomic write owns a shared validated identifier.
            .entity(EntityMutation::Update(EntityRecord::from_expanded(purged.current)))
            .history(purged.history)
            .precondition(Precondition::UnchangedSince(purged.modified_at))
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_snapshot::EntitySnapshot,
        expiry::{EntityExpiryBatchSize, EntityExpiryBatchSizeError, landed_ids, purge_write},
    };
    use andromeda_context_commit::{history_record::HistoryRecord, write_outcome::WriteOutcome};
    use andromeda_ngsi_ld_lexicon::uri::{Uri, UriParseError};
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};

    fn identifiers(raw: &[&str]) -> Result<Vec<Uri>, UriParseError> {
        raw.iter().map(|identifier| identifier.parse()).collect()
    }

    const fn cutoff() -> DateTime<Utc> {
        DateTime::<Utc>::UNIX_EPOCH
    }

    fn snapshot(payload: Value) -> EntitySnapshot {
        EntitySnapshot {
            payload,
            modified_at: cutoff(),
        }
    }

    fn expiring_instance(expires_at: &str) -> Value {
        json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
            "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@value": expires_at}]
        })
    }

    #[test]
    fn only_successful_purges_are_reported() -> Result<(), UriParseError> {
        let outcomes = vec![WriteOutcome::Updated, WriteOutcome::Superseded, WriteOutcome::Updated];
        pretty_assertions::assert_eq!(
            landed_ids(identifiers(&["urn:ngsi-ld:A", "urn:ngsi-ld:B", "urn:ngsi-ld:C"])?, &outcomes),
            identifiers(&["urn:ngsi-ld:A", "urn:ngsi-ld:C"])?
        );
        Ok(())
    }

    #[test]
    fn vanished_and_superseded_entities_are_not_reported() -> Result<(), UriParseError> {
        let outcomes = vec![WriteOutcome::NotFound, WriteOutcome::Superseded];
        assert!(landed_ids(identifiers(&["urn:ngsi-ld:A", "urn:ngsi-ld:B"])?, &outcomes).is_empty());
        Ok(())
    }

    #[test]
    fn concurrent_sweeps_report_one_semantic_attribute_deletion() -> Result<(), UriParseError> {
        let outcomes = vec![WriteOutcome::Updated, WriteOutcome::Superseded];
        let landed = landed_ids(identifiers(&["urn:ngsi-ld:A", "urn:ngsi-ld:A"])?, &outcomes);
        pretty_assertions::assert_eq!(landed, identifiers(&["urn:ngsi-ld:A"])?);
        Ok(())
    }

    #[test]
    fn a_past_due_instance_produces_an_optimistic_write_and_tombstone() -> Result<(), UriParseError> {
        let payload = json!([{"@id": "urn:ngsi-ld:A", "https://example.org/speed": [expiring_instance("1969-01-01T00:00:00Z")]}]);
        let identifier = "urn:ngsi-ld:A".parse::<Uri>()?;
        let write = purge_write(&identifier, snapshot(payload), cutoff());
        assert!(write.is_some());
        let Some(write) = write else {
            return Ok(());
        };
        pretty_assertions::assert_eq!(write.history.len(), 1);
        assert!(matches!(write.history[0], HistoryRecord::Tombstone(_)));
        assert!(write.precondition.is_unchanged_since());
        Ok(())
    }

    #[test]
    fn a_future_instance_produces_no_write() -> Result<(), UriParseError> {
        let payload = json!([{"@id": "urn:ngsi-ld:A", "https://example.org/speed": [expiring_instance("1971-01-01T00:00:00Z")]}]);
        let identifier = "urn:ngsi-ld:A".parse::<Uri>()?;
        assert!(purge_write(&identifier, snapshot(payload), cutoff()).is_none());
        Ok(())
    }

    #[test]
    fn expiry_batch_size_rejects_zero() {
        assert_eq!(EntityExpiryBatchSize::try_from(0), Err(EntityExpiryBatchSizeError::Zero));
    }
}
