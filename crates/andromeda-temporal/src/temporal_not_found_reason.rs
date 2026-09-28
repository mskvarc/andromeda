//! The concrete reasons a temporal resource is reported as `ResourceNotFound`.
//!
//! Every missing-resource failure of temporal policy is one variant here,
//! carrying the identifiers it names as typed fields, so
//! [`TemporalError::NotFound`](crate::error::TemporalError::NotFound) never holds
//! an ad-hoc string. Each `#[error]` renders lowercase client prose.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use thiserror::Error;

/// Why a temporal resource could not be found.
#[derive(Debug, Error)]
pub enum TemporalNotFoundReason {
    /// The entity has no Temporal Evolution.
    #[error("entity {id} not found")]
    Entity {
        /// The requested entity identifier.
        id: Uri,
    },
    /// The entity holds no such attribute.
    #[error("attribute {attribute} not found on entity {entity}")]
    AttributeOnEntity {
        /// The requested attribute.
        attribute: AttributeName,
        /// The entity the attribute was sought on.
        entity: Uri,
    },
    /// The attribute holds no instance with the requested `datasetId`.
    #[error("attribute {attribute} with datasetId {dataset_id} not found on entity {entity}")]
    AttributeDatasetOnEntity {
        /// The requested attribute.
        attribute: AttributeName,
        /// The `datasetId` that selected no instance.
        ///
        /// Boxed to keep this variant, which already carries the entity `Uri`,
        /// from being two `Uri`s wide and enlarging every `Result` in the crate.
        dataset_id: Box<Uri>,
        /// The entity the attribute was sought on.
        entity: Uri,
    },
    /// The attribute holds no default (no-`datasetId`) instance.
    #[error("attribute {attribute} default instance not found on entity {entity}")]
    AttributeDefaultInstanceOnEntity {
        /// The requested attribute.
        attribute: AttributeName,
        /// The entity the attribute was sought on.
        entity: Uri,
    },
    /// The attribute holds no instance with the requested `instanceId`.
    #[error("attribute instance {instance} not found")]
    AttributeInstance {
        /// The rendered instance identifier that selected no instance.
        instance: String,
    },
    /// The temporal window or the `attrs` selection left no instance of the
    /// entity at all (clause 5.7.3.4, step S1).
    #[error("no temporal data of entity {entity} matches the requested time interval and attributes")]
    NoInstanceSelected {
        /// The entity the restriction was applied to.
        entity: Uri,
    },
    /// A projection left no member of the entity's temporal representation
    /// (clause 5.7.3.5).
    #[error("no temporal representation of entity {entity} is left by the requested projection")]
    ProjectionLeftNothing {
        /// The entity the projection was applied to.
        entity: Uri,
    },
    /// The history store reported a missing resource.
    ///
    /// The text is relayed verbatim from the store adapter, which lives in a
    /// crate this one cannot depend on without a dependency cycle, so its reason
    /// arrives as opaque prose rather than a typed cause.
    #[error("{detail}")]
    Store {
        /// The store adapter's own missing-resource detail.
        detail: String,
    },
}
