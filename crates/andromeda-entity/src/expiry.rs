//! Applying clause 4.22 expiry through the ordinary atomic mutation boundary.

use crate::{
    EntityService,
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    modification_write::ModificationWrite,
    port::expiry::EntityExpirySource,
    write_target::WriteTarget,
};
#[cfg(test)]
use andromeda_context_commit::{atomic_write::AtomicWrite, entity_mutation::EntityMutation, entity_record::EntityRecord};
use andromeda_context_commit::{history_record::HistoryRecord, precondition::Precondition, write_outcome::WriteOutcome};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::{entity_state::EntityState, operation::expiry_removal::InstanceExpiry, reads::removed_instance::removed_attribute_instances};
#[cfg(test)]
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_lexicon::time::system_instant::SystemInstant;
use chrono::{DateTime, Utc};
use std::{num::NonZeroU32, sync::Arc};
use tracing::warn;

/// One entity-policy-owned bounded expiry operation.
pub struct EntityExpirySweep {
    /// The branch the sweep expires entities and attributes in.
    target: ContextTarget,
    /// The service whose atomic mutations remove what has expired.
    entity_policy: Arc<EntityService>,
    /// Finds entities and attribute instances whose expiry has passed.
    source: Arc<dyn EntityExpirySource>,
    /// Most candidates each phase of one sweep reads.
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
    ///
    /// A failure that belongs to one entity (its stored rows no longer read
    /// back, its write is refused or cannot be built) is logged and the entity
    /// is skipped, so it stays a candidate while the sweep reaches the
    /// candidates after it.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Persistence`] when the expiry source cannot be
    /// read or the sweep's writes cannot be committed, and the retryable
    /// store failures when the store is saturated, unreachable or contended
    /// on any entity, since every entity after it would fail alike.
    pub async fn execute(&self, cutoff: DateTime<Utc>) -> Result<EntityExpiryOutcome, EntityError> {
        let expired_entities = self
            .entity_policy
            .expire_due_entities(&self.target, self.source.as_ref(), cutoff, self.batch_size)
            .await?;
        let purged_attributes = self
            .entity_policy
            .purge_expired_attributes(&self.target, self.source.as_ref(), cutoff, self.batch_size)
            .await?;
        Ok(EntityExpiryOutcome {
            expired_entities,
            purged_attributes,
        })
    }
}

/// Non-zero maximum candidate count read by each phase of one expiry sweep.
#[derive(Clone, Copy, Debug, PartialEq, Eq, derive_more::From)]
pub struct EntityExpiryBatchSize(NonZeroU32);

impl EntityExpiryBatchSize {
    /// Return the validated candidate limit.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

/// What one bounded entity expiry reconciliation removed at its cutoff.
///
/// Clause 4.22 lets deletion lag `expiresAt` only by the clean-up cadence, so
/// the reconciliation always runs when it is scheduled; the tenant's pacer
/// already schedules it only once an `expiresAt` has fallen due.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityExpiryOutcome {
    /// Entity-level deletions that committed.
    pub expired_entities: usize,
    /// Entity payloads whose expired attribute instances were removed.
    pub purged_attributes: usize,
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
            match self.delete(target, WriteTarget::new(&entity_id, None)).await {
                Ok(()) => expired += 1,
                Err(EntityError::NotFound { identifier: _ }) => {}
                Err(error) => match failure_reach(&error) {
                    FailureReach::Sweep => return Err(error),
                    FailureReach::Entity => {
                        warn!(entity = %entity_id, %error, "entity expiry failed; the entity stays a candidate while the sweep goes on");
                    }
                },
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
        let now = SystemInstant::new(Utc::now());
        let mut writes = Vec::new();
        let mut pending = Vec::new();
        for (identifier, snapshot) in candidates {
            let Some(purged) = purged_state(snapshot, cutoff) else {
                warn!(entity = %identifier, "attribute-expiry candidate held no past-due instance");
                continue;
            };
            let ModificationWrite { mut write, events } = match self.modification_write(target, &identifier, &purged.read, purged.current, purged.history, now)
            {
                Ok(write) => write,
                Err(error) => match failure_reach(&error) {
                    FailureReach::Sweep => return Err(error),
                    FailureReach::Entity => {
                        warn!(entity = %identifier, %error, "attribute-expiry write could not be built; the entity stays a candidate while the sweep goes on");
                        continue;
                    }
                },
            };
            write.precondition = Precondition::UnchangedSince(purged.read.version);
            writes.push(write);
            pending.push((identifier, events));
        }
        if writes.is_empty() {
            return Ok(0);
        }

        let outcomes = self.writes.commit(target, now, writes).await?;
        let mut landed = 0;
        for ((identifier, events), outcome) in pending.into_iter().zip(&outcomes) {
            // The sweep writes back what the store already held, less the
            // expired instances, so a refusal means the stored entity no longer
            // reads as storable. The entity stays a candidate for the next
            // sweep while its neighbours are purged.
            if let WriteOutcome::Refused(refusal) = outcome {
                warn!(entity = %identifier, %refusal, "attribute-expiry write refused");
            }
            if let WriteOutcome::Updated(committed_at) = outcome {
                landed += 1;
                self.notifier.publish(target, *committed_at, events).await;
            }
        }
        Ok(landed)
    }
}

/// How far a failure met on one entity reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FailureReach {
    /// The store cannot serve the sweep right now, so every entity after this
    /// one would fail alike and the sweep stops until its next run.
    Sweep,
    /// The failure belongs to this entity, which is skipped.
    Entity,
}

/// How far `error`, met while expiring one entity, reaches.
const fn failure_reach(error: &EntityError) -> FailureReach {
    match error {
        EntityError::StoreSaturated | EntityError::StoreUnavailable | EntityError::TransactionContended => FailureReach::Sweep,
        EntityError::Validation(_)
        | EntityError::NotFound { .. }
        | EntityError::TypeNotFound { .. }
        | EntityError::AttributeNotFound { .. }
        | EntityError::MemberNotFound(_)
        | EntityError::AlreadyExists { .. }
        | EntityError::Contention { .. }
        | EntityError::OperationNotSupported(_)
        | EntityError::PayloadBudget(_)
        | EntityError::AttributeNesting(_)
        | EntityError::JsonLd(_)
        | EntityError::Query(_)
        | EntityError::Request(_)
        | EntityError::Representation(_)
        | EntityError::Commit(_)
        | EntityError::Persistence(_)
        | EntityError::Internal(_) => FailureReach::Entity,
    }
}

#[cfg(test)]
fn landed_ids(identifiers: Vec<Identifier>, outcomes: &[WriteOutcome]) -> Vec<Identifier> {
    identifiers
        .into_iter()
        .zip(outcomes)
        .filter(|(_, outcome)| outcome.is_updated())
        .map(|(identifier, _)| identifier)
        .collect()
}

/// One entity's state before and after its expired instances went.
struct PurgedState {
    /// The entity as it was read, with the version and the system attributes
    /// of that read.
    read: EntitySnapshot,
    /// The state without its expired instances.
    current: Arc<EntityState>,
    /// A tombstone for every instance that went.
    history: Vec<HistoryRecord>,
}

/// The entity `snapshot` becomes once every instance expired at `cutoff` is
/// gone (clause 4.22), or `None` when it held no such instance.
fn purged_state(snapshot: EntitySnapshot, cutoff: DateTime<Utc>) -> Option<PurgedState> {
    let mut purged = EntityState::clone(&snapshot.state); // clone: shares every attribute holding no expired instance
    match purged.remove_expired_instances(cutoff) {
        InstanceExpiry::NoneExpired => None,
        InstanceExpiry::Removed => {
            let history = removed_attribute_instances(&snapshot.state, &purged)
                .into_iter()
                .map(HistoryRecord::Tombstone)
                .collect();
            Some(PurgedState {
                read: snapshot,
                current: Arc::new(purged),
                history,
            })
        }
    }
}

#[cfg(test)]
fn purge_write(snapshot: EntitySnapshot, cutoff: DateTime<Utc>) -> Option<AtomicWrite> {
    let purged = purged_state(snapshot, cutoff)?;
    Some(
        AtomicWrite::builder()
            .entity_id(Arc::new(purged.current.id().clone())) // clone: the atomic write owns a shared validated identifier.
            .entity(EntityMutation::Update(
                EntityRecord::replacing(&purged.read.state, purged.current).expect("a projectable state"),
            ))
            .history(purged.history)
            .precondition(Precondition::UnchangedSince(purged.read.version))
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_snapshot::EntitySnapshot,
        error::EntityError,
        expiry::{EntityExpiryBatchSize, FailureReach, failure_reach, landed_ids, purge_write},
    };
    use andromeda_context_commit::{entity_version::EntityVersion, history_record::HistoryRecord, store_instant::StoreInstant, write_outcome::WriteOutcome};
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_lexicon::identifier::{Identifier, IdentifierParseError};
    use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};
    use std::{num::NonZeroU32, sync::Arc};

    fn identifiers(raw: &[&str]) -> Result<Vec<Identifier>, IdentifierParseError> {
        raw.iter().map(|identifier| identifier.parse()).collect()
    }

    const fn cutoff() -> DateTime<Utc> {
        DateTime::<Utc>::UNIX_EPOCH
    }

    fn snapshot(payload: &Value) -> EntitySnapshot {
        EntitySnapshot {
            state: Arc::new(EntityState::from_slice(&serde_json::to_vec(payload).expect("JSON")).expect("a stored entity")),
            version: EntityVersion::new(1, 1),
            sys_attrs: SysAttrsSnapshot {
                created_at: cutoff(),
                modified_at: cutoff(),
                expires_at: None,
            },
        }
    }

    fn expiring_instance(expires_at: &str) -> Value {
        json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
            "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": expires_at}]
        })
    }

    #[test]
    fn only_successful_purges_are_reported() -> Result<(), IdentifierParseError> {
        let outcomes = vec![
            WriteOutcome::Updated(StoreInstant::new(Utc::now())),
            WriteOutcome::Superseded,
            WriteOutcome::Updated(StoreInstant::new(Utc::now())),
        ];
        pretty_assertions::assert_eq!(
            landed_ids(identifiers(&["urn:ngsi-ld:A", "urn:ngsi-ld:B", "urn:ngsi-ld:C"])?, &outcomes),
            identifiers(&["urn:ngsi-ld:A", "urn:ngsi-ld:C"])?
        );
        Ok(())
    }

    #[test]
    fn vanished_and_superseded_entities_are_not_reported() -> Result<(), IdentifierParseError> {
        let outcomes = vec![WriteOutcome::NotFound, WriteOutcome::Superseded];
        let landed = landed_ids(identifiers(&["urn:ngsi-ld:A", "urn:ngsi-ld:B"])?, &outcomes);
        assert!(landed.is_empty(), "no identifier lands when every outcome failed: {landed:?}");
        Ok(())
    }

    #[test]
    fn concurrent_sweeps_report_one_semantic_attribute_deletion() -> Result<(), IdentifierParseError> {
        let outcomes = vec![WriteOutcome::Updated(StoreInstant::new(Utc::now())), WriteOutcome::Superseded];
        let landed = landed_ids(identifiers(&["urn:ngsi-ld:A", "urn:ngsi-ld:A"])?, &outcomes);
        pretty_assertions::assert_eq!(landed, identifiers(&["urn:ngsi-ld:A"])?);
        Ok(())
    }

    #[test]
    fn a_past_due_instance_produces_an_optimistic_write_and_tombstone() {
        let payload =
            json!([{"@id": "urn:ngsi-ld:A", "@type": ["https://example.org/T"], "https://example.org/speed": [expiring_instance("1969-01-01T00:00:00Z")]}]);
        let write = purge_write(snapshot(&payload), cutoff()).expect("the expired instance is purged");
        pretty_assertions::assert_eq!(write.history.len(), 1);
        assert!(matches!(write.history[0], HistoryRecord::Tombstone(_)));
        assert!(write.precondition.is_unchanged_since());
        let replaced = write.entity.record().and_then(|record| record.replaces.as_deref());
        assert!(
            replaced.is_some_and(|state| state.attributes().contains("https://example.org/speed")),
            "the purge states the state it read, so a store writes only the instances it removed"
        );
    }

    #[test]
    fn a_future_instance_produces_no_write() {
        let payload =
            json!([{"@id": "urn:ngsi-ld:A", "@type": ["https://example.org/T"], "https://example.org/speed": [expiring_instance("1971-01-01T00:00:00Z")]}]);
        assert!(purge_write(snapshot(&payload), cutoff()).is_none());
    }

    /// A store that cannot serve the sweep fails every entity after this one
    /// alike, so only those failures stop it.
    #[test]
    fn a_saturated_unreachable_or_contended_store_stops_the_sweep() {
        for error in [EntityError::StoreSaturated, EntityError::StoreUnavailable, EntityError::TransactionContended] {
            pretty_assertions::assert_eq!(failure_reach(&error), FailureReach::Sweep, "{error}");
        }
    }

    /// An entity stored in a form the broker no longer reads, or whose write
    /// cannot be built, is skipped while the sweep goes on.
    #[test]
    fn an_undecodable_entity_or_a_failed_write_is_skipped() -> Result<(), IdentifierParseError> {
        for error in [
            EntityError::Persistence(PersistenceFailure::Undecodable),
            EntityError::Persistence(PersistenceFailure::ConstraintViolated),
            EntityError::contention(&"urn:ngsi-ld:A".parse()?, 3),
        ] {
            pretty_assertions::assert_eq!(failure_reach(&error), FailureReach::Entity, "{error}");
        }
        Ok(())
    }

    #[test]
    fn the_expiry_batch_size_keeps_the_configured_count() {
        assert_eq!(EntityExpiryBatchSize::from(NonZeroU32::MIN.saturating_add(999)).get(), 1_000);
    }
}
