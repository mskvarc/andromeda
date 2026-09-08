//! Failures raised by the entity- and attribute-level mutation operations.

use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, uri::Uri};
use iri_rs::IriBuf;
use thiserror::Error;

/// Failures of the entity-level mutation operations.
///
/// Every variant carries the offending attribute and dataset identifiers as
/// separate fields rather than baked into a message, so the HTTP layer can map
/// each one onto the `ProblemDetails` type NGSI-LD clause 5.5.2 requires instead
/// of reporting one undifferentiated failure.
///
/// The attribute IRI is an [`IriBuf`] and the `datasetId` a [`Uri`], the domain
/// types they carry across the API boundary.
///
/// A [`Uri`] is 88 bytes against the 56 an [`IriBuf`] occupies, and this enum is
/// the `Err` of every mutation entry point: carrying the `datasetId` inline made
/// the widest variant 144 bytes, so every success path paid the failure's width.
/// [`DatasetNotFound`](Self::DatasetNotFound) therefore boxes it.
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
    /// An attribute member did not satisfy the clause-4.6.2 name grammar.
    #[error("invalid attribute name")]
    InvalidAttributeName,
    /// An attribute instance declared a type outside the clause-5.2.4 set.
    #[error("attribute instance has an invalid attribute type")]
    InvalidAttributeType,
    /// The entity carries no attribute under this IRI.
    #[error("attribute {attribute} not found")]
    AttributeNotFound {
        /// Expanded IRI of the attribute that was looked up.
        attribute: IriBuf,
    },
    /// The attribute exists but holds no instance array to target.
    #[error("attribute {attribute} has no instances")]
    AttributeHasNoInstances {
        /// Expanded IRI of the attribute that was looked up.
        attribute: IriBuf,
    },
    /// The attribute exists but has no instance with the requested `datasetId`.
    #[error("attribute {attribute} with datasetId {dataset_id} not found")]
    DatasetNotFound {
        /// Expanded IRI of the attribute that was looked up.
        attribute: IriBuf,
        /// The `datasetId` that selected no instance, boxed to keep this
        /// variant from widening the whole enum.
        dataset_id: Box<Uri>,
    },
    /// The attribute exists but has no default (no-`datasetId`) instance.
    #[error("attribute {attribute} default instance not found")]
    DefaultInstanceNotFound {
        /// Expanded IRI of the attribute that was looked up.
        attribute: IriBuf,
    },
    /// The fragment declares a type that the stored instance does not share.
    #[error("attribute type mismatch for {attribute}: fragment declares {fragment_types:?}, instance is {instance_types:?}")]
    AttributeTypeMismatch {
        /// Expanded IRI of the attribute that was looked up.
        attribute: IriBuf,
        /// Expanded type IRIs declared by the incoming fragment.
        fragment_types: Vec<AttributeType>,
        /// Expanded type IRIs carried by the stored instance.
        instance_types: Vec<AttributeType>,
    },
}
