//! Focused failures owned by snapshot policy and persistence ports.

use andromeda_entity::error::EntityError;
use andromeda_federation::error::FederationError;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_request::error::ParameterError;
use andromeda_temporal::error::TemporalError;
use thiserror::Error;

/// A snapshot lifecycle, execution, or persistence failure.
#[derive(Debug, Error)]
pub enum SnapshotError {
    /// The request violates snapshot semantics.
    #[error("invalid snapshot: {detail}")]
    Validation {
        /// Human-readable validation detail.
        detail: String,
    },
    /// The requested snapshot is absent or not queryable.
    #[error("snapshot not found: {identifier}")]
    NotFound {
        /// Missing snapshot identifier.
        identifier: Uri,
    },
    /// The requested identifier is already occupied.
    #[error("snapshot already exists: {identifier}")]
    AlreadyExists {
        /// Conflicting snapshot identifier.
        identifier: Uri,
    },
    /// The Snapshot remained in preparation beyond the scoped-operation deadline.
    #[error("snapshot preparation did not complete before the scoped-operation deadline: {identifier}")]
    PreparationTimeout {
        /// Snapshot that remained in preparation.
        identifier: Uri,
    },
    /// The Snapshot's stored branch is behind this binary's branch relations
    /// because its upgrade has not run or has failed, so it cannot be read,
    /// cloned or delivered from until the upgrade succeeds.
    #[error("snapshot {identifier} is quarantined: its stored branch has not been upgraded to this broker's schema")]
    Quarantined {
        /// The quarantined snapshot.
        identifier: Uri,
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
    #[error("snapshot persistence failed: {detail}")]
    Persistence {
        /// Backend-neutral diagnostic detail.
        detail: String,
    },
    /// A connection pool was saturated before the operation could run.
    ///
    /// Backpressure, not a fault: no backend became free within the acquire
    /// deadline, so the well-formed request is shed and can be retried. It is
    /// answered 503 with `Retry-After`, never the 500 a broker malfunction earns.
    #[error("the datastore connection pool is saturated; retry shortly")]
    StoreSaturated,
    /// A transaction lost a deadlock or serialization race and was rolled back.
    ///
    /// Not a fault: PostgreSQL aborts the whole transaction (SQLSTATE 40P01 or
    /// 40001) and picks one contender as the victim, so the operation is safely
    /// retryable from a fresh transaction. Reaping a Snapshot's branch schema
    /// takes locks that can deadlock against the notification delivery worker
    /// draining that branch, which this classifies so the store can retry it and,
    /// if the budget is exhausted, the client is shed a 503 rather than a 500.
    #[error("the snapshot store transaction was rolled back by contention; retry shortly")]
    Contended,
    /// Policy produced an invalid internal value or transition.
    #[error("snapshot invariant failed: {detail}")]
    Internal {
        /// Internal invariant diagnostic.
        detail: String,
    },
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
    /// Construct a semantic validation failure.
    pub fn validation(detail: impl Into<String>) -> SnapshotError {
        SnapshotError::Validation { detail: detail.into() }
    }

    /// Construct a missing snapshot failure from a validated identifier.
    #[must_use]
    pub fn not_found(identifier: &Uri) -> SnapshotError {
        SnapshotError::NotFound {
            identifier: identifier.clone(), // clone: the error owns the borrowed snapshot identifier.
        }
    }

    /// Construct an identifier conflict.
    #[must_use]
    pub fn already_exists(identifier: &Uri) -> SnapshotError {
        SnapshotError::AlreadyExists {
            identifier: identifier.clone(), // clone: the error owns the borrowed snapshot identifier.
        }
    }

    /// Construct a bounded preparation wait failure.
    #[must_use]
    pub fn preparation_timeout(identifier: &Uri) -> SnapshotError {
        SnapshotError::PreparationTimeout {
            identifier: identifier.clone(), // clone: the error owns the borrowed snapshot identifier.
        }
    }

    /// Construct a quarantined-branch refusal.
    #[must_use]
    pub fn quarantined(identifier: &Uri) -> SnapshotError {
        SnapshotError::Quarantined {
            identifier: identifier.clone(), // clone: the error owns the borrowed snapshot identifier.
        }
    }

    /// Construct a backend-neutral persistence failure.
    pub fn persistence(detail: impl Into<String>) -> SnapshotError {
        SnapshotError::Persistence { detail: detail.into() }
    }

    /// Construct an invariant failure.
    pub fn internal(detail: impl Into<String>) -> SnapshotError {
        SnapshotError::Internal { detail: detail.into() }
    }

    /// Whether a fresh attempt at the operation that failed could succeed.
    ///
    /// A lost deadlock or serialization race, a saturated pool and a store
    /// failure (a dropped connection among them) are transient, whether the
    /// snapshot store or an embedded entity or temporal operation met them.
    /// Everything else is decided by the request or by the stored state, so
    /// repeating it repeats the failure.
    #[must_use]
    pub const fn recurrence(&self) -> Recurrence {
        match self {
            SnapshotError::Contended | SnapshotError::StoreSaturated | SnapshotError::Persistence { .. } => Recurrence::Transient,
            SnapshotError::Entity(error) => entity_recurrence(error),
            SnapshotError::Temporal(error) => temporal_recurrence(error),
            SnapshotError::Validation { .. }
            | SnapshotError::NotFound { .. }
            | SnapshotError::AlreadyExists { .. }
            | SnapshotError::PreparationTimeout { .. }
            | SnapshotError::Quarantined { .. }
            | SnapshotError::Federation(_)
            | SnapshotError::Request(_)
            | SnapshotError::Representation(_)
            | SnapshotError::Internal { .. } => Recurrence::Persistent,
        }
    }
}

/// Whether a fresh attempt at an entity operation could succeed: only a lost
/// race or a saturated pool clears on its own.
pub(crate) const fn entity_recurrence(error: &EntityError) -> Recurrence {
    if matches!(error, EntityError::TransactionContended | EntityError::StoreSaturated) {
        Recurrence::Transient
    } else {
        Recurrence::Persistent
    }
}

/// Whether a fresh attempt at a temporal operation could succeed: only a lost
/// race or a saturated pool clears on its own.
pub(crate) const fn temporal_recurrence(error: &TemporalError) -> Recurrence {
    if matches!(error, TemporalError::TransactionContended | TemporalError::StoreSaturated) {
        Recurrence::Transient
    } else {
        Recurrence::Persistent
    }
}

#[cfg(test)]
mod tests {
    use crate::error::{Recurrence, SnapshotError};
    use andromeda_entity::error::EntityError;
    use andromeda_temporal::error::TemporalError;
    use pretty_assertions::assert_eq;

    #[test]
    fn contention_saturation_and_store_failures_are_transient() {
        for error in [
            SnapshotError::Contended,
            SnapshotError::StoreSaturated,
            SnapshotError::persistence("connection reset"),
            SnapshotError::from(EntityError::TransactionContended),
            SnapshotError::from(EntityError::StoreSaturated),
            SnapshotError::from(TemporalError::TransactionContended),
            SnapshotError::from(TemporalError::StoreSaturated),
        ] {
            assert_eq!(error.recurrence(), Recurrence::Transient, "{error}");
        }
    }

    #[test]
    fn invalid_transitions_and_broken_invariants_are_persistent() {
        for error in [
            SnapshotError::validation("Invalid status transition"),
            SnapshotError::internal("no staging branch"),
            SnapshotError::from(EntityError::internal("broken invariant")),
            SnapshotError::from(TemporalError::internal("broken invariant")),
        ] {
            assert_eq!(error.recurrence(), Recurrence::Persistent, "{error}");
        }
    }
}
