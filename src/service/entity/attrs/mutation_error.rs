//! Mapping an expanded-entity mutation failure onto a ProblemDetails type.
//!
//! Lives here rather than beside [`AndromedaError`] because the model crate must
//! not know the broker's transport-aware error type, and attribute mutation is
//! what raises these failures.
//!
//! [`AndromedaError`]: crate::error::AndromedaError

use crate::error::AndromedaError;
use andromeda_ngsi_ld_model::expanded::mutation::error::ExpandedEntityError;

impl From<ExpandedEntityError> for AndromedaError {
    /// Map each mutation failure onto the NGSI-LD ProblemDetails type clause
    /// 5.5.2 requires: a malformed request body is `BadRequestData` (400),
    /// while a well-formed request naming an attribute, dataset or instance
    /// the entity does not have is `ResourceNotFound` (404).
    fn from(error: ExpandedEntityError) -> AndromedaError {
        match error {
            ExpandedEntityError::EntityNotExpanded
            | ExpandedEntityError::PatchNotExpanded
            | ExpandedEntityError::AttributesNotObject
            | ExpandedEntityError::InvalidAttributeName
            | ExpandedEntityError::InvalidAttributeType
            | ExpandedEntityError::AttributeTypeMismatch { .. } => AndromedaError::entity_validation(error.to_string()),
            ExpandedEntityError::AttributeNotFound { .. }
            | ExpandedEntityError::AttributeHasNoInstances { .. }
            | ExpandedEntityError::DatasetNotFound { .. }
            | ExpandedEntityError::DefaultInstanceNotFound { .. } => AndromedaError::entity_not_found(error.to_string()),
        }
    }
}
