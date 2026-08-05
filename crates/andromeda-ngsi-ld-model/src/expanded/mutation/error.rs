//! Failures raised by the entity- and attribute-level mutation operations.

use thiserror::Error;

/// Failures of the entity-level mutation operations.
///
/// Every variant carries the offending attribute and dataset identifiers as
/// separate fields rather than baked into a message, so the HTTP layer can map
/// each one onto the ProblemDetails type NGSI-LD clause 5.5.2 requires instead
/// of reporting one undifferentiated failure.
///
/// The identifiers are plain strings because that is what the expanded payload
/// itself uses for attribute keys; when the attribute map gains an IRI key
/// type, these fields follow it.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ExpandedEntityError {
    /// The entity value was not an expanded JSON-LD node (nor an array wrapping one).
    #[error("entity data is not a valid JSON-LD expanded value")]
    EntityNotExpanded,
    /// The patch value was not an expanded JSON-LD node.
    #[error("patch data is not a valid JSON-LD expanded value")]
    PatchNotExpanded,
    /// The attribute fragment was not a JSON object.
    #[error("attributes must be a JSON object")]
    AttributesNotObject,
    /// The entity carries no attribute under this IRI.
    #[error("attribute {attribute} not found")]
    AttributeNotFound {
        /// Expanded IRI of the attribute that was looked up.
        attribute: String,
    },
    /// The attribute exists but holds no instance array to target.
    #[error("attribute {attribute} has no instances")]
    AttributeHasNoInstances {
        /// Expanded IRI of the attribute that was looked up.
        attribute: String,
    },
    /// The attribute exists but has no instance with the requested `datasetId`.
    #[error("attribute {attribute} with datasetId {dataset_id} not found")]
    DatasetNotFound {
        /// Expanded IRI of the attribute that was looked up.
        attribute: String,
        /// The `datasetId` that selected no instance.
        dataset_id: String,
    },
    /// The attribute exists but has no default (no-`datasetId`) instance.
    #[error("attribute {attribute} default instance not found")]
    DefaultInstanceNotFound {
        /// Expanded IRI of the attribute that was looked up.
        attribute: String,
    },
    /// The fragment declares a type that the stored instance does not share.
    #[error("attribute type mismatch for {attribute}: fragment declares {fragment_types:?}, instance is {instance_types:?}")]
    AttributeTypeMismatch {
        /// Expanded IRI of the attribute that was looked up.
        attribute: String,
        /// Expanded type IRIs declared by the incoming fragment.
        fragment_types: Vec<String>,
        /// Expanded type IRIs carried by the stored instance.
        instance_types: Vec<String>,
    },
}
