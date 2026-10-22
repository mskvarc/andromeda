//! PostgreSQL anchor persistence and delivery state for temporal append intents.
//!
//! The store's enqueue, run inside the entity anchor transaction, lives in
//! [`enqueue`]; the claim, high-water, release-probe, removal, release and
//! retry statements in [`lease_statements`]; the statements keeping each
//! Entity's intents in commit order in [`queue_order`]; the mutation barrier's
//! wait on another owner's lease in [`release_wait`]; the port implementations
//! here. An application first refuses a set holding an intent behind an
//! earlier one of its Entity still queued outside it, then runs the history
//! statements of `andromeda-postgresql-temporal-store` over the claimed rows
//! and removes them under the lease in the same transaction.
//!
//! The queue lives only in the live branch: a Snapshot branch records its
//! history in the write's own transaction, so nothing here names a branch.

/// Enqueueing temporal append intents inside the entity anchor transaction.
mod enqueue;
/// The claim, high-water, release-probe, lease-guarded removal, release and
/// retry statements.
mod lease_statements;
/// The statements keeping each Entity's queued intents in commit order.
mod queue_order;
/// Waiting for another owner's lease on one entity's intents to end.
mod release_wait;

use crate::intent_store::{
    lease_statements::{ClaimScope, applied_removal_statement, claim_statement, last_intent_statement, release_statement, retry_statement},
    queue_order::behind_earlier_statement,
    release_wait::await_release,
};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder, write_id::WriteId};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_pool::persistence_classification::classify;
use andromeda_postgresql_seaquery::statement::Statement;
use andromeda_postgresql_temporal_store::intent_application::{apply_intents_in_tx, intent_source::IntentSource};
use andromeda_telemetry::statement::{StatementName, observe_statement};
use andromeda_temporal::{
    append_intent::TemporalIntentKey,
    error::TemporalError,
    port::{
        temporal_intent_application::{TemporalIntentApplication, TemporalIntentApplicationOutcome},
        temporal_intent_queue::{ClaimedTemporalIntent, EntityRelease, TemporalIntentBatchSize, TemporalIntentQueue, TemporalIntentRetry},
    },
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row, postgres::PgRow};
use std::time::Duration;
use uuid::Uuid;

/// The statement name the intent claim is observed under.
const INTENT_CLAIM: StatementName = StatementName("temporal-intent-claim");
/// The statement name intent retry is observed under.
const INTENT_RETRY: StatementName = StatementName("temporal-intent-retry");
/// The statement name the release of held-back intents is observed under.
const INTENT_RELEASE: StatementName = StatementName("temporal-intent-release");
/// The name an intent application, from its first statement to its commit, is
/// observed under.
const INTENT_APPLICATION: StatementName = StatementName("temporal-intent-application");

/// PostgreSQL-backed pending temporal intent queue for one tenant.
#[derive(Clone, Debug)]
pub struct PostgresTemporalIntentStore {
    /// Tenant anchor pool.
    pool: PgPool,
}

impl PostgresTemporalIntentStore {
    /// Construct a pending-intent store over the tenant anchor pool.
    #[must_use]
    pub fn new(pool: &PgPool) -> PostgresTemporalIntentStore {
        PostgresTemporalIntentStore {
            pool: pool.clone(), // clone: SQLx pools are shared reference-counted handles.
        }
    }

    /// Decode one claimed row: the intent's identity, never its entries.
    fn decode(row: &PgRow) -> Result<ClaimedTemporalIntent, TemporalError> {
        let raw_key: i64 = row.try_get("intent_key").map_err(|error| persistence(&error))?;
        let key = TemporalIntentKey::new(raw_key).map_err(|error| TemporalError::from(PersistenceFailure::Undecodable.record(&error)))?;
        let write_id: Uuid = row.try_get("write_id").map_err(|error| persistence(&error))?;
        let entity_id: String = row.try_get("entity_id").map_err(|error| persistence(&error))?;
        let entity_id = entity_id
            .parse::<Identifier>()
            .map_err(|error| TemporalError::from(PersistenceFailure::Undecodable.record(&error)))?;
        let attempts: i32 = row.try_get("attempt_count").map_err(|error| persistence(&error))?;
        let attempts = u32::try_from(attempts).map_err(|error| TemporalError::from(PersistenceFailure::Undecodable.record(&error)))?;
        Ok(ClaimedTemporalIntent {
            key,
            write_id: WriteId::from_uuid(write_id),
            entity_id,
            attempts: attempts.saturating_sub(1),
        })
    }

    /// Claim an ordered bounded batch of the intents `scope` admits.
    async fn claim_matching(
        &self,
        owner: LeaseHolder,
        scope: ClaimScope<'_>,
        limit: TemporalIntentBatchSize,
        lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalIntent>, TemporalError> {
        let rows = observe_statement(
            INTENT_CLAIM,
            claim_statement(owner, scope, limit.get(), lease_until).to_query().fetch_all(&self.pool),
        )
        .await
        .map_err(|error| persistence(&error))?;
        let mut claimed = rows
            .iter()
            .map(PostgresTemporalIntentStore::decode)
            .collect::<Result<Vec<_>, TemporalError>>()?;
        claimed.sort_by_key(|claim| claim.key);
        Ok(claimed)
    }

    /// Apply `intents` and remove them under `owner`'s live lease, in one
    /// transaction that commits only when no earlier intent of their Entities
    /// is queued outside them and every intent is still held.
    async fn apply_held(&self, owner: LeaseHolder, intents: &[ClaimedTemporalIntent]) -> Result<TemporalIntentApplicationOutcome, TemporalError> {
        let keys: Vec<i64> = intents.iter().map(|intent| intent.key.get()).collect();
        let mut tx = self.pool.begin().await.map_err(|error| persistence(&error))?;
        let behind: bool = behind_earlier_statement(&keys)
            .to_query_scalar()
            .fetch_one(&mut *tx)
            .await
            .map_err(|error| persistence(&error))?;
        if behind {
            tx.rollback().await.map_err(|error| persistence(&error))?;
            return Ok(TemporalIntentApplicationOutcome::Behind);
        }
        let source = IntentSource::claimed(&mut tx, &keys).await.map_err(TemporalError::from)?;
        apply_intents_in_tx(&mut tx, &source, &ContextTarget::Live).await.map_err(TemporalError::from)?;
        let removed = applied_removal_statement(&keys, owner)
            .to_query()
            .execute(&mut *tx)
            .await
            .map_err(|error| persistence(&error))?
            .rows_affected();
        if usize::try_from(removed).ok() != Some(keys.len()) {
            tx.rollback().await.map_err(|error| persistence(&error))?;
            return Ok(TemporalIntentApplicationOutcome::LeaseLost);
        }
        tx.commit().await.map_err(|error| persistence(&error))?;
        Ok(TemporalIntentApplicationOutcome::Applied)
    }
}

#[async_trait]
impl TemporalIntentQueue for PostgresTemporalIntentStore {
    async fn claim(&self, owner: LeaseHolder, limit: TemporalIntentBatchSize, lease_until: DateTime<Utc>) -> Result<Vec<ClaimedTemporalIntent>, TemporalError> {
        self.claim_matching(owner, ClaimScope::Due, limit, lease_until).await
    }

    async fn claim_for_entity(
        &self,
        owner: LeaseHolder,
        entity_id: &Identifier,
        through: TemporalIntentKey,
        limit: TemporalIntentBatchSize,
        lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalIntent>, TemporalError> {
        self.claim_matching(owner, ClaimScope::EntityThrough { entity_id, through }, limit, lease_until)
            .await
    }

    async fn last_intent_for(&self, entity_id: &Identifier) -> Result<Option<TemporalIntentKey>, TemporalError> {
        let newest: Option<i64> = last_intent_statement(entity_id)
            .to_query_scalar()
            .fetch_one(&self.pool)
            .await
            .map_err(|error| persistence(&error))?;
        newest
            .map(|key| TemporalIntentKey::new(key).map_err(|error| TemporalError::from(PersistenceFailure::Undecodable.record(&error))))
            .transpose()
    }

    async fn await_entity_release(&self, entity_id: &Identifier, through: TemporalIntentKey, wait: Duration) -> Result<EntityRelease, TemporalError> {
        await_release(&self.pool, entity_id, through, wait).await
    }

    async fn retry(&self, owner: LeaseHolder, intent: &ClaimedTemporalIntent, retry: TemporalIntentRetry) -> Result<LeaseAcknowledgement, TemporalError> {
        let result = observe_statement(
            INTENT_RETRY,
            retry_statement(retry.eligible_at, &retry.summary, intent.key.get(), intent.write_id.as_uuid(), owner)
                .to_query()
                .execute(&self.pool),
        )
        .await
        .map_err(|error| persistence(&error))?;
        held(result.rows_affected())
    }

    async fn release(&self, owner: LeaseHolder, intents: &[ClaimedTemporalIntent]) -> Result<LeaseAcknowledgement, TemporalError> {
        if intents.is_empty() {
            return Ok(LeaseAcknowledgement::Applied);
        }
        let keys: Vec<i64> = intents.iter().map(|intent| intent.key.get()).collect();
        let released = observe_statement(INTENT_RELEASE, release_statement(&keys, owner).to_query().execute(&self.pool))
            .await
            .map_err(|error| persistence(&error))?
            .rows_affected();
        Ok(if usize::try_from(released).ok() == Some(keys.len()) {
            LeaseAcknowledgement::Applied
        } else {
            LeaseAcknowledgement::Lost
        })
    }
}

#[async_trait]
impl TemporalIntentApplication for PostgresTemporalIntentStore {
    async fn apply(&self, owner: LeaseHolder, intents: &[ClaimedTemporalIntent]) -> Result<TemporalIntentApplicationOutcome, TemporalError> {
        if intents.is_empty() {
            return Ok(TemporalIntentApplicationOutcome::Applied);
        }
        observe_statement(INTENT_APPLICATION, self.apply_held(owner, intents)).await
    }
}

/// Whether a lease-guarded statement reached the one row it targeted.
///
/// Zero rows means the lease lapsed and another worker now owns the row; more
/// than one cannot happen, since the statement names the row's key.
fn held(rows_affected: u64) -> Result<LeaseAcknowledgement, TemporalError> {
    match rows_affected {
        0 => Ok(LeaseAcknowledgement::Lost),
        1 => Ok(LeaseAcknowledgement::Applied),
        _more => Err(TemporalError::from(PersistenceFailure::ContractBroken.record_bare())),
    }
}

/// Classify one anchor-store failure at the temporal port boundary.
#[track_caller]
pub(super) fn persistence(error: &sqlx::Error) -> TemporalError {
    TemporalError::from(classify(error).record(error))
}

#[cfg(test)]
mod tests {
    use crate::intent_store::held;
    use andromeda_context_commit::lease_acknowledgement::LeaseAcknowledgement;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_statement_reaching_no_row_reports_the_lease_as_lost() {
        assert_eq!(held(0).expect("a lost lease is an outcome"), LeaseAcknowledgement::Lost);
        assert_eq!(held(1).expect("a held lease is an outcome"), LeaseAcknowledgement::Applied);
        assert!(held(2).is_err());
    }
}
