//! Failures produced by entity policy and its persistence ports.

use andromeda_context_commit::error::CommitError;
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    entity_type_name::EntityTypeName,
    ngsi_ld_urn::{AttributeListIdParseError, EntityTypeListIdParseError},
    uri::Uri,
};
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_ngsi_ld_request::error::ParameterError;
use derive_more::IsVariant;
use thiserror::Error;

/// A failure while validating, reading, querying, mutating, or persisting an entity.
#[derive(Debug, Error, IsVariant)]
pub enum EntityError {
    /// The entity payload or operation is invalid.
    #[error("invalid entity operation: {detail}")]
    Validation {
        /// Human-readable validation detail.
        detail: String,
    },
    /// The requested entity does not exist.
    #[error("entity not found: {identifier}")]
    NotFound {
        /// Missing entity identifier.
        identifier: Uri,
    },
    /// The requested entity type does not exist.
    #[error("entity type not found: {name}")]
    TypeNotFound {
        /// Missing entity type name.
        name: EntityTypeName,
    },
    /// The requested attribute does not exist.
    #[error("attribute not found: {name}")]
    AttributeNotFound {
        /// Missing attribute name.
        name: AttributeName,
    },
    /// A requested entity member or dataset instance does not exist.
    #[error("entity member not found: {detail}")]
    MemberNotFound {
        /// Human-readable missing-member detail.
        detail: String,
    },
    /// An entity already uses the requested identifier.
    #[error("entity already exists: {identifier}")]
    AlreadyExists {
        /// Conflicting entity identifier.
        identifier: Uri,
    },
    /// Concurrent writers superseded the mutation on every attempt it was given.
    ///
    /// The submitted content is not at fault and nothing about it needs
    /// changing: the entity is simply being written faster than a
    /// read-modify-write can land against it.
    #[error("entity {identifier} is being modified concurrently: the write lost its precondition on all {attempts} attempts and can be retried unchanged")]
    Contention {
        /// Contended entity identifier.
        identifier: Uri,
        /// Attempts spent before the broker gave up.
        attempts: u32,
    },
    /// The selected storage capability cannot answer the operation safely.
    #[error("entity operation is not supported: {detail}")]
    OperationNotSupported {
        /// Unsupported capability detail.
        detail: String,
    },
    /// JSON-LD processing failed.
    #[error(transparent)]
    JsonLd(#[from] JsonLdError),
    /// NGSI-LD query compilation failed.
    #[error(transparent)]
    Query(#[from] QueryError),
    /// Query-parameter interpretation failed.
    #[error(transparent)]
    Request(#[from] ParameterError),
    /// JSON representation processing failed.
    #[error(transparent)]
    Representation(#[from] serde_json::Error),
    /// Identifier generation failed.
    #[error(transparent)]
    Identifier(#[from] urn_rs::Error),
    /// An entity-type-list identifier has invalid NGSI-LD URN form.
    #[error(transparent)]
    EntityTypeListIdentifier(#[from] EntityTypeListIdParseError),
    /// An attribute-list identifier has invalid NGSI-LD URN form.
    #[error(transparent)]
    AttributeListIdentifier(#[from] AttributeListIdParseError),
    /// An atomic context write failed for a reason the client cannot act on.
    ///
    /// A commit that refused the submitted content never reaches this variant:
    /// [`From<CommitError>`] turns that refusal into [`EntityError::Validation`],
    /// which is what clause 5.5.6 reports as `BadRequestData`.
    #[error(transparent)]
    Commit(CommitError),
    /// A persistence adapter could not complete the operation.
    #[error("entity persistence failed: {detail}")]
    Persistence {
        /// Adapter-specific diagnostic detail.
        detail: String,
    },
    /// An internal entity invariant could not be satisfied.
    #[error("entity invariant failed: {detail}")]
    Internal {
        /// Internal diagnostic detail.
        detail: String,
    },
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// The commit port reports a refused submission separately from its own faults,
/// and collapsing both into one variant would answer an unusable request with
/// `InternalError` and invite the client to retry it forever (clause 5.5.6).
impl From<CommitError> for EntityError {
    fn from(error: CommitError) -> EntityError {
        match error {
            CommitError::Submission { detail } => EntityError::Validation { detail },
            failure @ (CommitError::DuplicateEntity { .. }
            | CommitError::Persistence { .. }
            | CommitError::Invariant { .. }
            | CommitError::UnexpectedOutcomeCount { .. }) => EntityError::Commit(failure),
        }
    }
}

impl EntityError {
    /// Construct an entity-validation failure.
    pub fn validation(detail: impl Into<String>) -> EntityError {
        EntityError::Validation { detail: detail.into() }
    }

    /// Construct a missing-entity failure.
    pub fn not_found(identifier: &Uri) -> EntityError {
        EntityError::NotFound {
            identifier: identifier.clone(), // clone: the error owns the borrowed request identifier.
        }
    }

    /// Construct a missing-entity-type failure.
    pub fn type_not_found(name: &EntityTypeName) -> EntityError {
        EntityError::TypeNotFound {
            name: name.clone(), // clone: the error owns the borrowed request type.
        }
    }

    /// Construct a missing-attribute failure.
    pub fn attribute_not_found(name: &AttributeName) -> EntityError {
        EntityError::AttributeNotFound {
            name: name.clone(), // clone: the error owns the borrowed request attribute.
        }
    }

    /// Construct a missing entity member or dataset-instance failure.
    pub fn member_not_found(detail: impl Into<String>) -> EntityError {
        EntityError::MemberNotFound { detail: detail.into() }
    }

    /// Construct an identifier-conflict failure.
    pub fn already_exists(identifier: &Uri) -> EntityError {
        EntityError::AlreadyExists {
            identifier: identifier.clone(), // clone: the error owns the borrowed request identifier.
        }
    }

    /// Construct a write-contention failure over the attempts it consumed.
    pub fn contention(identifier: &Uri, attempts: u32) -> EntityError {
        EntityError::Contention {
            identifier: identifier.clone(), // clone: the error owns the borrowed request identifier.
            attempts,
        }
    }

    /// Construct an unsupported-capability failure.
    pub fn operation_not_supported(detail: impl Into<String>) -> EntityError {
        EntityError::OperationNotSupported { detail: detail.into() }
    }

    /// Construct a persistence-adapter failure.
    pub fn persistence(detail: impl Into<String>) -> EntityError {
        EntityError::Persistence { detail: detail.into() }
    }

    /// Construct an internal-invariant failure.
    pub fn internal(detail: impl Into<String>) -> EntityError {
        EntityError::Internal { detail: detail.into() }
    }
}
