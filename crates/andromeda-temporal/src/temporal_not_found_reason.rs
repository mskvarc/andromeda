//! The concrete reasons a temporal resource is reported as `ResourceNotFound`.
//!
//! Every missing-resource failure of temporal policy is one variant here,
//! carrying the identifiers it names as typed fields, so
//! [`TemporalError::NotFound`](crate::error::TemporalError::NotFound) never holds
//! an ad-hoc string. Each `#[error]` renders lowercase client prose.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, instance_identity::InstanceIdentity};
use thiserror::Error;

/// Why a temporal resource could not be found.
#[derive(Debug, Error)]
pub enum TemporalNotFoundReason {
    /// The entity has no Temporal Evolution.
    #[error("entity {id} not found")]
    Entity {
        /// The requested entity identifier.
        id: Identifier,
    },
    /// The entity holds no such attribute.
    #[error("attribute {attribute} not found on entity {entity}")]
    AttributeOnEntity {
        /// The requested attribute.
        attribute: AttributeName,
        /// The entity the attribute was sought on.
        entity: Identifier,
    },
    /// The attribute holds no instance with the requested `datasetId`.
    #[error("attribute {attribute} with datasetId {dataset_id} not found on entity {entity}")]
    AttributeDatasetOnEntity {
        /// The requested attribute.
        attribute: AttributeName,
        /// The `datasetId` that selected no instance.
        ///
        /// Boxed to keep this variant, which already carries the entity `Identifier`,
        /// from being two `Identifier`s wide and enlarging every `Result` in the crate.
        dataset_id: Box<Identifier>,
        /// The entity the attribute was sought on.
        entity: Identifier,
    },
    /// The attribute holds no default (no-`datasetId`) instance.
    #[error("attribute {attribute} default instance not found on entity {entity}")]
    AttributeDefaultInstanceOnEntity {
        /// The requested attribute.
        attribute: AttributeName,
        /// The entity the attribute was sought on.
        entity: Identifier,
    },
    /// The attribute holds no instance with the requested `instanceId`.
    #[error("attribute instance {} not found", .instance.rendered())]
    AttributeInstance {
        /// The instance identifier that selected no instance.
        instance: InstanceIdentity,
    },
    /// The entity has no Temporal Evolution, and the write declares no entity
    /// type to create one with (clause 4.5.1).
    #[error("the Temporal Evolution of entity {entity} does not exist and the write declares no entity type to create it with")]
    UntypedEvolution {
        /// The entity the write addressed.
        entity: Identifier,
    },
    /// The temporal window or the `attrs` selection left no instance of the
    /// entity at all (clause 5.7.3.4, step S1).
    #[error("no temporal data of entity {entity} matches the requested time interval and attributes")]
    NoInstanceSelected {
        /// The entity the restriction was applied to.
        entity: Identifier,
    },
    /// A projection left no member of the entity's temporal representation
    /// (clause 5.7.3.5).
    #[error("no temporal representation of entity {entity} is left by the requested projection")]
    ProjectionLeftNothing {
        /// The entity the projection was applied to.
        entity: Identifier,
    },
}
