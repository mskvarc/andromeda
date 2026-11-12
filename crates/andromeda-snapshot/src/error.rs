//! Focused failures owned by snapshot policy and persistence ports.

use crate::{snapshot_invariant::SnapshotInvariant, snapshot_validation_reason::SnapshotValidationReason};
use andromeda_entity::error::EntityError;
use andromeda_federation::error::FederationError;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_request::error::ParameterError;
use andromeda_persistence_failure::{persistence_failure::PersistenceFailure, retryability::RetryCause};
use andromeda_temporal::error::TemporalError;
use thiserror::Error;

/// A snapshot lifecycle, execution, or persistence failure.
#[derive(Debug, Error)]
pub enum SnapshotError {
    /// The request violates snapshot semantics.
    #[error("invalid snapshot: {0}")]
    Validation(#[from] SnapshotValidationReason),
    /// The requested snapshot is absent or not queryable.
    #[error("snapshot not found: {identifier}")]
    NotFound {
        /// Missing snapshot identifier.
        identifier: Identifier,
    },
    /// The requested identifier is already occupied.
    #[error("snapshot already exists: {identifier}")]
    AlreadyExists {
        /// Conflicting snapshot identifier.
        identifier: Identifier,
    },
    /// The Snapshot remained in preparation beyond the scoped-operation deadline.
    #[error("snapshot preparation did not complete before the scoped-operation deadline: {identifier}")]
    PreparationTimeout {
        /// Snapshot that remained in preparation.
        identifier: Identifier,
    },
    /// The Snapshot's stored branch is behind this binary's branch relations
    /// because its upgrade has not run or has failed, so it cannot be read,
    /// cloned or delivered from until the upgrade succeeds.
    #[error("snapshot {identifier} is quarantined: its stored branch has not been upgraded to this broker's schema")]
    Quarantined {
        /// The quarantined snapshot.
        identifier: Identifier,
    },
    /// Entity query policy failed during preparation.
    ///
    /// Boxed: these embedded policy errors are the widest members, and inlining
    /// them would enlarge every snapshot `Result`. The [`From`] impls below keep
    /// `?` ergonomic.
    #[error(transparent)]
    Entity(Box<EntityError>),
    /// Temporal query policy failed during preparation.
    #[error(transparent)]
    Temporal(Box<TemporalError>),
    /// Federation policy failed during preparation.
    #[error(transparent)]
    Federation(Box<FederationError>),
    /// A canonical request parameter was invalid.
    #[error(transparent)]
    Request(#[from] ParameterError),
    /// Snapshot JSON representation failed.
    #[error(transparent)]
    Representation(#[from] serde_json::Error),
    /// Persistence could not complete the operation.
    #[error("snapshot persistence failed: {0}")]
    Persistence(#[source] PersistenceFailure),
    /// A connection pool was saturated before the operation could run.
    ///
    /// Backpressure, not a fault: no backend became free within the acquire
    /// deadline, so the well-formed request is shed and can be retried. It is
    /// answered 503 with `Retry-After`, never the 500 a broker malfunction earns.
    #[error("{}", RetryCause::Saturated)]
    StoreSaturated,
    /// The datastore could not be reached, or the connection to it was lost.
    ///
    /// Not a fault of the request: nothing of it is known to have persisted, so
    /// the identical request can be sent again once the store is back. It is
    /// answered 503 with `Retry-After`, never the 500 a broker malfunction earns.
    #[error("{}", RetryCause::Unreachable)]
    StoreUnavailable,
    /// A transaction lost a deadlock or serialization race and was rolled back.
    ///
    /// Not a fault: PostgreSQL aborts the whole transaction (SQLSTATE 40P01 or
    /// 40001) and picks one contender as the victim, so the operation is safely
    /// retryable from a fresh transaction. Reaping a Snapshot's branch schema
    /// takes locks that can deadlock against the notification delivery worker
    /// draining that branch, which this classifies so the store can retry it and,
    /// if the budget is exhausted, the client is shed a 503 rather than a 500.
    #[error("{}", RetryCause::Contended)]
    Contended,
    /// Policy produced an invalid internal value or transition.
    #[error("snapshot invariant failed: {0}")]
    Internal(#[from] SnapshotInvariant),
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// A saturated store, a transaction rolled back under contention and a store
/// that cannot be reached are conditions the client can retry, so each keeps
/// its own retryable category; only a fault becomes
/// [`SnapshotError::Persistence`].
impl From<PersistenceFailure> for SnapshotError {
    fn from(failure: PersistenceFailure) -> SnapshotError {
        match failure {
            PersistenceFailure::Saturated => SnapshotError::StoreSaturated,
            PersistenceFailure::Unavailable => SnapshotError::StoreUnavailable,
            PersistenceFailure::RolledBack => SnapshotError::Contended,
            fault @ (PersistenceFailure::Misconfigured
            | PersistenceFailure::ConstraintViolated
            | PersistenceFailure::StatementRejected
            | PersistenceFailure::RecordMissing
            | PersistenceFailure::Undecodable
            | PersistenceFailure::Unencodable
            | PersistenceFailure::StatementUnbuildable
            | PersistenceFailure::SchemaMismatch
            | PersistenceFailure::ContractBroken) => SnapshotError::Persistence(fault),
        }
    }
}

/// Whether a fresh attempt at a failed operation could succeed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Recurrence {
    /// A fresh attempt can succeed: the failure came from contention,
    /// backpressure or the store connection, not from what was asked.
    Transient,
    /// Every fresh attempt fails the same way.
    Persistent,
}

/// Box the embedded entity policy error, keeping `?` ergonomic while snapshot
/// `Result`s stay small.
impl From<EntityError> for SnapshotError {
    fn from(error: EntityError) -> SnapshotError {
        SnapshotError::Entity(Box::new(error))
    }
}

/// Box the embedded temporal policy error, keeping `?` ergonomic while snapshot
/// `Result`s stay small.
impl From<TemporalError> for SnapshotError {
    fn from(error: TemporalError) -> SnapshotError {
        SnapshotError::Temporal(Box::new(error))
    }
}

/// Box the embedded federation policy error, keeping `?` ergonomic while snapshot
/// `Result`s stay small.
impl From<FederationError> for SnapshotError {
    fn from(error: FederationError) -> SnapshotError {
        SnapshotError::Federation(Box::new(error))
    }
}

impl SnapshotError {
    /// Construct a missing snapshot failure from a validated identifier.
    #[must_use]
    pub fn not_found(identifier: &Identifier) -> SnapshotError {
        SnapshotError::NotFound {
            identifier: identifier.clone(), // clone: the error owns the borrowed snapshot identifier.
        }
    }

    /// Construct an identifier conflict.
    #[must_use]
    pub fn already_exists(identifier: &Identifier) -> SnapshotError {
        SnapshotError::AlreadyExists {
            identifier: identifier.clone(), // clone: the error owns the borrowed snapshot identifier.
        }
    }

    /// Construct a bounded preparation wait failure.
    #[must_use]
    pub fn preparation_timeout(identifier: &Identifier) -> SnapshotError {
        SnapshotError::PreparationTimeout {
            identifier: identifier.clone(), // clone: the error owns the borrowed snapshot identifier.
        }
    }

    /// Construct a quarantined-branch refusal.
    #[must_use]
    pub fn quarantined(identifier: &Identifier) -> SnapshotError {
        SnapshotError::Quarantined {
            identifier: identifier.clone(), // clone: the error owns the borrowed snapshot identifier.
        }
    }

    /// Whether a fresh attempt at the operation that failed could succeed.
    ///
    /// A lost deadlock or serialization race, a saturated pool, an unreachable
    /// store and a store failure are transient, whether the
    /// snapshot store or an embedded entity or temporal operation met them.
    /// Everything else is decided by the request or by the stored state, so
    /// repeating it repeats the failure.
    #[must_use]
    pub const fn recurrence(&self) -> Recurrence {
        match self {
            SnapshotError::Contended | SnapshotError::StoreSaturated | SnapshotError::StoreUnavailable | SnapshotError::Persistence(_) => Recurrence::Transient,
            SnapshotError::Entity(error) => entity_recurrence(error),
            SnapshotError::Temporal(error) => temporal_recurrence(error),
            SnapshotError::Validation(_)
            | SnapshotError::NotFound { .. }
            | SnapshotError::AlreadyExists { .. }
            | SnapshotError::PreparationTimeout { .. }
            | SnapshotError::Quarantined { .. }
            | SnapshotError::Federation(_)
            | SnapshotError::Request(_)
            | SnapshotError::Representation(_)
            | SnapshotError::Internal(_) => Recurrence::Persistent,
        }
    }
}

/// Whether a fresh attempt at an entity operation could succeed: only a lost
/// race, a saturated pool or an unreachable store clears on its own.
pub(crate) const fn entity_recurrence(error: &EntityError) -> Recurrence {
    if matches!(
        error,
        EntityError::TransactionContended | EntityError::StoreSaturated | EntityError::StoreUnavailable
    ) {
        Recurrence::Transient
    } else {
        Recurrence::Persistent
    }
}

/// Whether a fresh attempt at a temporal operation could succeed: only a lost
/// race, a saturated pool, an unreachable store or queued history another
/// owner is still applying clears on its own.
pub(crate) const fn temporal_recurrence(error: &TemporalError) -> Recurrence {
    if matches!(
        error,
        TemporalError::TransactionContended | TemporalError::StoreSaturated | TemporalError::StoreUnavailable | TemporalError::HistoryUnsettled(_)
    ) {
        Recurrence::Transient
    } else {
        Recurrence::Persistent
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::{Recurrence, SnapshotError},
        snapshot_invariant::SnapshotInvariant,
        snapshot_validation_reason::SnapshotValidationReason,
    };
    use andromeda_entity::{entity_invariant::EntityInvariant, error::EntityError};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_temporal::{error::TemporalError, temporal_invariant::TemporalInvariant};
    use pretty_assertions::assert_eq;

    #[test]
    fn contention_saturation_unavailability_and_store_failures_are_transient() {
        for error in [
            SnapshotError::Contended,
            SnapshotError::StoreSaturated,
            SnapshotError::from(PersistenceFailure::Unavailable),
            SnapshotError::from(PersistenceFailure::StatementRejected),
            SnapshotError::from(EntityError::TransactionContended),
            SnapshotError::from(EntityError::StoreSaturated),
            SnapshotError::from(EntityError::StoreUnavailable),
            SnapshotError::from(TemporalError::TransactionContended),
            SnapshotError::from(TemporalError::StoreSaturated),
            SnapshotError::from(TemporalError::StoreUnavailable),
        ] {
            assert_eq!(error.recurrence(), Recurrence::Transient, "{error}");
        }
    }

    #[test]
    fn invalid_transitions_and_broken_invariants_are_persistent() {
        for error in [
            SnapshotError::from(SnapshotValidationReason::IdImmutable),
            SnapshotError::from(SnapshotInvariant::QuotaWithoutEvictable),
            SnapshotError::from(EntityError::from(EntityInvariant::BatchPreparationFailed)),
            SnapshotError::from(TemporalError::from(TemporalInvariant::FoldCreatedNoEntity)),
        ] {
            assert_eq!(error.recurrence(), Recurrence::Persistent, "{error}");
        }
    }

    /// A store that cannot be reached is retryable, answered 503 rather than
    /// the 500 of a fault; a configuration no retry mends stays a fault.
    #[test]
    fn an_unreachable_store_keeps_its_retryable_category() {
        assert!(matches!(SnapshotError::from(PersistenceFailure::Unavailable), SnapshotError::StoreUnavailable));
        assert!(matches!(
            SnapshotError::from(PersistenceFailure::Misconfigured),
            SnapshotError::Persistence(PersistenceFailure::Misconfigured)
        ));
    }
}
