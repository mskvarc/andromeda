//! Mapping an expanded-entity mutation failure onto a ProblemDetails type.
//!
//! Lives here rather than beside [`EntityError`] because the model crate must
//! not know the broker's transport-aware error type, and attribute mutation is
//! what raises these failures.
//!
//! [`EntityError`]: crate::error::EntityError

use crate::{entity_validation_reason::EntityValidationReason, error::EntityError};
use andromeda_ngsi_ld_model::expanded::mutation::error::ExpandedEntityError;

impl From<ExpandedEntityError> for EntityError {
    /// Map each mutation failure onto the NGSI-LD `ProblemDetails` type clause
    /// 5.5.2 requires: a malformed request body is `BadRequestData` (400),
    /// while a well-formed request naming an attribute, dataset or instance
    /// the entity does not have is `ResourceNotFound` (404).
    ///
    /// The typed [`ExpandedEntityError`] is carried through unchanged rather than
    /// stringified: it renders its own client-facing message, and keeping it
    /// typed lets the mapping stay total over its variants.
    fn from(error: ExpandedEntityError) -> EntityError {
        match error {
            malformed @ (ExpandedEntityError::EntityNotExpanded
            | ExpandedEntityError::PatchNotExpanded
            | ExpandedEntityError::AttributesNotObject
            | ExpandedEntityError::InvalidAttributeName { .. }
            | ExpandedEntityError::InvalidAttributeType
            | ExpandedEntityError::AttributeTypeMismatch { .. }) => EntityError::Validation(EntityValidationReason::Expanded(malformed)),
            missing @ (ExpandedEntityError::AttributeNotFound { .. }
            | ExpandedEntityError::AttributeHasNoInstances { .. }
            | ExpandedEntityError::DatasetNotFound { .. }
            | ExpandedEntityError::DefaultInstanceNotFound { .. }) => EntityError::MemberNotFound(missing),
        }
    }
}
