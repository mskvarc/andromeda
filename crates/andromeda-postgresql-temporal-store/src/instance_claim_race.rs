//! The invariant that one client-supplied `instanceId` names one attribute
//! instance across every sibling `datasetId` stream of an attribute, and what
//! a statement meeting it means.
//!
//! Clause 4.5.7 makes `instanceId` unique to one attribute instance, not to
//! one dataset stream of it, which is the same scope instance PATCH and
//! DELETE already search (`temporal_attribute_streams` joined on
//! `entity_key` and `attribute_name_key`, not on `stream_key` alone).
//! `attribute_history` cannot carry that uniqueness itself: it is a
//! TimescaleDB hypertable partitioned on `recorded_at`, and a unique index
//! there must include the partitioning column, which would not even help --
//! two rows claiming the same `instanceId` at two different write instants
//! would still both pass it. `attribute_history_instance_claims` is an
//! ordinary table that carries the invariant on the database's own behalf.
//!
//! The constraint never refuses a client's write. One write naming an
//! `instanceId` twice is refused while its history is interpreted, before any
//! statement runs, and an application releases the claim of every instance it
//! replaces before staking its own. What is left to meet the constraint is a
//! claim another transaction committed after this one released: a concurrent
//! writer of the same `instanceId`, or an earlier queued intent of the same
//! Entity applied in the same batch. Either is contention. A rerun sees the
//! committed instance and replaces it, so the last writer's value is the one
//! kept, and a drain batch is applied again one intent at a time, in commit
//! order.

use crate::error::TemporalStoreError;
use thiserror::Error;

/// The name of the primary key that carries this invariant.
pub const PK_ATTRIBUTE_HISTORY_INSTANCE_CLAIMS: &str = "pk_attribute_history_instance_claims";

/// Another transaction claimed an `instanceId` this one was staking, after
/// this one released the claims of the instances it replaces.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("a concurrent write claimed the same instanceId of the same attribute first")]
pub struct InstanceClaimRace;

/// Lift a statement failure into the race when the instance-claim primary key
/// is what refused the write.
#[must_use]
pub fn claim_write_failure(error: sqlx::Error) -> TemporalStoreError {
    if let sqlx::Error::Database(database) = &error
        && database.constraint() == Some(PK_ATTRIBUTE_HISTORY_INSTANCE_CLAIMS)
    {
        return InstanceClaimRace.into();
    }
    TemporalStoreError::Database(error)
}

#[cfg(test)]
mod tests {
    use crate::{
        error::TemporalStoreError,
        instance_claim_race::{InstanceClaimRace, PK_ATTRIBUTE_HISTORY_INSTANCE_CLAIMS, claim_write_failure},
    };
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_postgresql_pool::fake_database_error::FakeDatabaseError;
    use andromeda_temporal::error::TemporalError;

    #[test]
    fn a_failure_unrelated_to_the_claim_constraint_surfaces_as_an_ordinary_database_error() {
        let error = claim_write_failure(sqlx::Error::RowNotFound);
        assert!(matches!(error, TemporalStoreError::Database(sqlx::Error::RowNotFound)));
    }

    #[test]
    fn a_unique_violation_on_another_constraint_stays_a_database_error() {
        let error = claim_write_failure(FakeDatabaseError::new("duplicate key", "23505").with_constraint("pk_entities").into_sqlx());
        assert!(matches!(error, TemporalStoreError::Database(_)), "{error}");
    }

    /// Clause 4.5.7 keeps one instance per `instanceId`; a claim a concurrent
    /// writer committed first is contention the committer reruns, so the
    /// rerun replaces that writer's instance instead of refusing the client.
    #[test]
    fn a_claim_taken_by_a_concurrent_writer_is_contention() {
        let error = claim_write_failure(
            FakeDatabaseError::new("duplicate key", "23505")
                .with_constraint(PK_ATTRIBUTE_HISTORY_INSTANCE_CLAIMS)
                .into_sqlx(),
        );
        assert!(matches!(error, TemporalStoreError::InstanceClaimRace(InstanceClaimRace)), "{error}");
        pretty_assertions::assert_eq!(error.failure_class(), Some(PersistenceFailure::RolledBack));
        assert!(matches!(TemporalError::from(error), TemporalError::TransactionContended));
    }
}
