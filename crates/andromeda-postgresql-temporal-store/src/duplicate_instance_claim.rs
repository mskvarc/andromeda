//! The invariant that one client-supplied `instanceId` names one attribute
//! instance across every sibling `datasetId` stream of an attribute.
//!
//! Clause 4.5.7 makes `instanceId` unique to one attribute instance, not to
//! one dataset stream of it, which is the same scope instance PATCH and
//! DELETE already search (`temporal_attribute_streams` joined on
//! `temporal_entity_key` and `attribute_name_id`, not on `stream_id` alone).
//! `attribute_history` cannot carry that uniqueness itself: it is a
//! TimescaleDB hypertable partitioned on `recorded_at`, and a unique index
//! there must include the partitioning column, which would not even help --
//! two rows claiming the same `instanceId` at two different write instants
//! would still both pass it. `attribute_history_instance_claims` is an
//! ordinary table that carries the invariant on the database's own behalf, so
//! a race two application-level checks could never see -- two rows in one
//! batch, or two concurrent batches -- is refused by a real constraint
//! instead of surviving as two stored rows.

use crate::error::TemporalStoreError;
use thiserror::Error;

/// The name of the primary key that carries this invariant.
///
/// A statement failing on it is refused as [`DuplicateInstanceClaimError`]
/// rather than surfaced as an opaque persistence fault, because the violation
/// is caused by the client's own write, not by a broker malfunction.
pub const PK_ATTRIBUTE_HISTORY_INSTANCE_CLAIMS: &str = "pk_attribute_history_instance_claims";

/// A client-supplied `instanceId` claimed more than one attribute instance of
/// the same attribute at once.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("instanceId is already claimed by another instance of the same attribute, and clause 4.5.7 makes it unique to one")]
pub struct DuplicateInstanceClaimError;

/// Lift a statement failure into the typed invariant when the instance-claim
/// primary key is what refused the write.
pub fn claim_write_failure(error: sqlx::Error) -> TemporalStoreError {
    if let sqlx::Error::Database(database) = &error
        && database.constraint() == Some(PK_ATTRIBUTE_HISTORY_INSTANCE_CLAIMS)
    {
        return DuplicateInstanceClaimError.into();
    }
    TemporalStoreError::Database(error)
}

#[cfg(test)]
mod tests {
    use crate::{duplicate_instance_claim::claim_write_failure, error::TemporalStoreError};

    #[test]
    fn a_failure_unrelated_to_the_claim_constraint_surfaces_as_an_ordinary_database_error() {
        let error = claim_write_failure(sqlx::Error::RowNotFound);
        assert!(matches!(error, TemporalStoreError::Database(sqlx::Error::RowNotFound)));
    }
}
