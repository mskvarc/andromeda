//! Mapping an expanded-entity mutation failure onto a ProblemDetails type.
//!
//! Lives here rather than beside [`EntityError`] because the model crate must
//! not know the broker's transport-aware error type, and attribute mutation is
//! what raises these failures.
//!
//! [`EntityError`]: crate::error::EntityError

use crate::error::EntityError;
use andromeda_ngsi_ld_model::expanded::mutation::error::ExpandedEntityError;

impl From<ExpandedEntityError> for EntityError {
    /// Map each mutation failure onto the NGSI-LD `ProblemDetails` type clause
    /// 5.5.2 requires: a malformed request body is `BadRequestData` (400),
    /// while a well-formed request naming an attribute, dataset or instance
    /// the entity does not have is `ResourceNotFound` (404).
    fn from(error: ExpandedEntityError) -> EntityError {
        match error {
            ExpandedEntityError::EntityNotExpanded
            | ExpandedEntityError::PatchNotExpanded
            | ExpandedEntityError::AttributesNotObject
            | ExpandedEntityError::InvalidAttributeName
            | ExpandedEntityError::InvalidAttributeType
            | ExpandedEntityError::AttributeTypeMismatch { .. } => EntityError::validation(error.to_string()),
            ExpandedEntityError::AttributeNotFound { .. }
            | ExpandedEntityError::AttributeHasNoInstances { .. }
            | ExpandedEntityError::DatasetNotFound { .. }
            | ExpandedEntityError::DefaultInstanceNotFound { .. } => EntityError::member_not_found(error.to_string()),
        }
    }
}
