//! Why a write could not be applied to an entity.
//!
//! Every variant carries the offending attribute and dataset identifiers as
//! typed fields rather than baked into a message, so the transport layer maps
//! each one onto the `ProblemDetails` type clause 5.5.2 requires: a member the
//! entity lacks is a `ResourceNotFound`, everything else a `BadRequestData`.

use crate::refusal::EntityRefusal;
use andromeda_json_literal::admission::JsonLiteralError;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier};
use thiserror::Error;

/// A write that cannot be applied.
#[derive(Debug, Error)]
pub enum MutationError {
    /// The entity carries no attribute of this name.
    #[error("attribute {attribute} not found")]
    AttributeNotFound {
        /// Expanded name of the attribute that was looked up.
        attribute: AttributeName,
    },
    /// The attribute exists but has no instance with the requested
    /// `datasetId`.
    #[error("attribute {attribute} with datasetId {dataset_id} not found")]
    DatasetNotFound {
        /// Expanded name of the attribute that was looked up.
        attribute: AttributeName,
        /// The `datasetId` that selected no instance, boxed to keep this
        /// variant from widening the whole enum.
        dataset_id: Box<Identifier>,
    },
    /// The attribute exists but has no default (no-`datasetId`) instance.
    #[error("attribute {attribute} default instance not found")]
    DefaultInstanceNotFound {
        /// Expanded name of the attribute that was looked up.
        attribute: AttributeName,
    },
    /// The fragment declares a family the stored instance does not have
    /// (clause 5.6.4.4: "it is not allowed to change the type of an
    /// Attribute").
    #[error("attribute type mismatch for {attribute}: fragment declares [{fragment_type}], instance is [{instance_type}]")]
    AttributeTypeMismatch {
        /// Expanded name of the attribute that was looked up.
        attribute: AttributeName,
        /// The family the fragment declares.
        fragment_type: AttributeType,
        /// The family of the stored instance.
        instance_type: AttributeType,
    },
    /// The written instance cannot become entity state.
    #[error(transparent)]
    Refused(Box<EntityRefusal>),
    /// A JSON literal could not be written back after a merge, which no
    /// admitted literal causes.
    #[error("a merged JSON literal could not be written: {0}")]
    LiteralMerge(#[source] JsonLiteralError),
}

impl MutationError {
    /// The clause 5.6.4.4 `ResourceNotFound` for an instance that was
    /// selected but is not there: the one carrying `dataset_id`, or the
    /// default instance.
    #[must_use]
    pub fn missing_instance(attribute: &AttributeName, dataset_id: Option<&Identifier>) -> MutationError {
        match dataset_id {
            Some(dataset_id) => MutationError::DatasetNotFound {
                attribute: attribute.clone(),             // clone: the error outlives the borrowed request
                dataset_id: Box::new(dataset_id.clone()), // clone: the error outlives the borrowed request
            },
            None => MutationError::DefaultInstanceNotFound {
                attribute: attribute.clone(), // clone: the error outlives the borrowed request
            },
        }
    }
}

impl From<EntityRefusal> for MutationError {
    // `thiserror`'s `#[from]` converts from the field type, which is the box;
    // the refusal itself is what every producer holds.
    fn from(refusal: EntityRefusal) -> MutationError {
        MutationError::Refused(Box::new(refusal))
    }
}
