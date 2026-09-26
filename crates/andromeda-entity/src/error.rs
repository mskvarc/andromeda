//! Failures produced by entity policy and its persistence ports.

use crate::{entity_diagnostic::EntityDiagnostic, entity_operation_unsupported::EntityOperationUnsupported, entity_validation_reason::EntityValidationReason};
use andromeda_context_commit::error::CommitError;
use andromeda_jsonld::error::JsonLdError;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    entity_type_name::EntityTypeName,
    ngsi_ld_urn::{AttributeListIdParseError, EntityTypeListIdParseError},
    uri::Uri,
};
use andromeda_ngsi_ld_model::expanded::mutation::error::ExpandedEntityError;
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_ngsi_ld_request::error::ParameterError;
use derive_more::IsVariant;
use std::fmt::Display;
use thiserror::Error;

/// A failure while validating, reading, querying, mutating, or persisting an entity.
#[derive(Debug, Error, IsVariant)]
pub enum EntityError {
    /// The entity payload or operation is invalid.
    #[error("invalid entity operation: {0}")]
    Validation(#[from] EntityValidationReason),
    /// The requested entity does not exist.
    #[error("entity {identifier} not found")]
    NotFound {
        /// Missing entity identifier.
        identifier: Uri,
    },
    /// The requested entity type does not exist.
    #[error("entity type {name} not found")]
    TypeNotFound {
        /// Missing entity type name.
        name: EntityTypeName,
    },
    /// The requested attribute does not exist.
    #[error("attribute {name} not found")]
    AttributeNotFound {
        /// Missing attribute name.
        name: AttributeName,
    },
    /// A requested entity member or dataset instance does not exist.
    #[error("entity member not found: {0}")]
    MemberNotFound(ExpandedEntityError),
    /// An entity already uses the requested identifier.
    #[error("entity {identifier} already exists")]
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
    #[error("entity operation is not supported: {0}")]
    OperationNotSupported(EntityOperationUnsupported),
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
    #[error("entity persistence failed: {0}")]
    Persistence(EntityDiagnostic),
    /// A connection pool was saturated before the operation could run.
    ///
    /// Backpressure, not a fault: no backend became free within the acquire
    /// deadline, so the well-formed request is shed and can be retried. It is
    /// answered 503 with `Retry-After`, never the 500 a broker malfunction earns.
    #[error("the datastore connection pool is saturated; retry shortly")]
    StoreSaturated,
    /// The datastore rolled the write back to break a deadlock or a
    /// serialization conflict with a concurrent transaction, on every attempt
    /// the committer made.
    ///
    /// Not a fault: nothing of the write persisted, so the identical request
    /// can be sent again. It is answered 503 with `Retry-After`, never the 500
    /// a broker malfunction earns.
    #[error("the write was rolled back by a concurrent transaction; retry shortly")]
    TransactionContended,
    /// An internal entity invariant could not be satisfied.
    #[error("entity invariant failed: {0}")]
    Internal(EntityDiagnostic),
}

/// Manual rather than `#[from]`: the conversion classifies instead of wrapping.
/// The commit port reports a refused submission separately from its own faults,
/// and collapsing both into one variant would answer an unusable request with
/// `InternalError` and invite the client to retry it forever (clause 5.5.6).
impl From<CommitError> for EntityError {
    fn from(error: CommitError) -> EntityError {
        match error {
            // The commit port cannot depend on this crate's error types without a
            // cycle, so a refused submission arrives as opaque prose relayed into
            // the typed validation reason rather than as a typed cause.
            CommitError::Submission { detail } => EntityError::Validation(EntityValidationReason::CommitRefused { detail }),
            // Saturation is backpressure the client can retry, so it keeps its
            // own category rather than being folded into the internal-fault
            // `Commit` variant.
            CommitError::StoreSaturated => EntityError::StoreSaturated,
            CommitError::TransactionContended => EntityError::TransactionContended,
            failure @ (CommitError::DuplicateEntity { .. }
            | CommitError::Persistence { .. }
            | CommitError::Invariant { .. }
            | CommitError::UnexpectedOutcomeCount { .. }) => EntityError::Commit(failure),
        }
    }
}

impl EntityError {
    /// Construct a missing-entity failure.
    #[must_use]
    pub fn not_found(identifier: &Uri) -> EntityError {
        EntityError::NotFound {
            identifier: identifier.clone(), // clone: the error owns the borrowed request identifier.
        }
    }

    /// Construct a missing-entity-type failure.
    #[must_use]
    pub fn type_not_found(name: &EntityTypeName) -> EntityError {
        EntityError::TypeNotFound {
            name: name.clone(), // clone: the error owns the borrowed request type.
        }
    }

    /// Construct a missing-attribute failure.
    #[must_use]
    pub fn attribute_not_found(name: &AttributeName) -> EntityError {
        EntityError::AttributeNotFound {
            name: name.clone(), // clone: the error owns the borrowed request attribute.
        }
    }

    /// Construct an identifier-conflict failure.
    #[must_use]
    pub fn already_exists(identifier: &Uri) -> EntityError {
        EntityError::AlreadyExists {
            identifier: identifier.clone(), // clone: the error owns the borrowed request identifier.
        }
    }

    /// Construct a write-contention failure over the attempts it consumed.
    #[must_use]
    pub fn contention(identifier: &Uri, attempts: u32) -> EntityError {
        EntityError::Contention {
            identifier: identifier.clone(), // clone: the error owns the borrowed request identifier.
            attempts,
        }
    }

    /// Construct a persistence-adapter failure from its masked diagnostic source.
    ///
    /// The detail is captured for operators and masked before any client sees
    /// it, so this takes `impl Display` — a typed adapter error is handed over
    /// directly rather than pre-rendered to a string at the call site.
    pub fn persistence(source: impl Display) -> EntityError {
        EntityError::Persistence(EntityDiagnostic::new(source))
    }

    /// Construct an internal-invariant failure from its masked diagnostic source.
    pub fn internal(source: impl Display) -> EntityError {
        EntityError::Internal(EntityDiagnostic::new(source))
    }
}
