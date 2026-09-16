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
    /// Policy produced an invalid internal value or transition.
    #[error("snapshot invariant failed: {detail}")]
    Internal {
        /// Internal invariant diagnostic.
        detail: String,
    },
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

    /// Construct a backend-neutral persistence failure.
    pub fn persistence(detail: impl Into<String>) -> SnapshotError {
        SnapshotError::Persistence { detail: detail.into() }
    }

    /// Construct an invariant failure.
    pub fn internal(detail: impl Into<String>) -> SnapshotError {
        SnapshotError::Internal { detail: detail.into() }
    }
}
