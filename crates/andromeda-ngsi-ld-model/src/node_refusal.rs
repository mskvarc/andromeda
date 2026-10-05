//! Why a compact node is not read as the typed entity or attribute instance
//! it might have been.
//!
//! A refused attribute instance is no failure: the member that held it is
//! carried verbatim instead, exactly as a decoded document carries it. A node
//! that has no JSON form is another matter, and is told apart so the reader of
//! such a tree can write it in full and read it again.

use crate::attribute::geo_property::TypedGeometryRefusal;
use andromeda_json_literal::admission::JsonLiteralError;
use andromeda_ngsi_ld_geometry::geojson::GeometryError;
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, reference::NgsiLdReferenceError};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use derive_more::Display;
use std::error::Error;
use thiserror::Error;

/// A member of an entity or of an attribute instance a refusal names, by the
/// name it is written under (clauses 4.5 and 5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum RefusedMember {
    /// The entity's `id`.
    #[display("id")]
    Id,
    /// The entity's `type`.
    #[display("type")]
    Type,
    /// The entity's `scope`.
    #[display("{}", compact_properties::SCOPE)]
    Scope,
    /// The member holding a family's value.
    #[display("{}", _0.compact_value_key())]
    Value(AttributeType),
    /// The member holding a family's previous value.
    #[display("{}", _0.previous_key())]
    Previous(AttributeType),
    /// `unitCode`.
    #[display("{}", compact_properties::UNIT_CODE)]
    UnitCode,
    /// `valueType`.
    #[display("{}", compact_properties::VALUE_TYPE)]
    ValueType,
    /// `objectType`.
    #[display("{}", compact_properties::OBJECT_TYPE)]
    ObjectType,
    /// A Relationship's inline `entity`.
    #[display("{}", compact_properties::ENTITY)]
    Entity,
    /// A `ListRelationship`'s inline `entityList`.
    #[display("{}", compact_properties::ENTITY_LIST)]
    EntityList,
    /// `createdAt`.
    #[display("{}", compact_properties::CREATED_AT)]
    CreatedAt,
    /// `datasetId`.
    #[display("{}", compact_properties::DATASET_ID)]
    DatasetId,
    /// `deletedAt`.
    #[display("{}", compact_properties::DELETED_AT)]
    DeletedAt,
    /// `expiresAt`.
    #[display("{}", compact_properties::EXPIRES_AT)]
    ExpiresAt,
    /// `instanceId`.
    #[display("{}", compact_properties::INSTANCE_ID)]
    InstanceId,
    /// `modifiedAt`.
    #[display("{}", compact_properties::MODIFIED_AT)]
    ModifiedAt,
    /// `observedAt`.
    #[display("{}", compact_properties::OBSERVED_AT)]
    ObservedAt,
}

/// Why a compact node is no typed entity or attribute instance.
#[derive(Debug, Error)]
pub enum NodeRefusal {
    /// The node is no JSON object.
    #[error("an entity or an attribute instance is a JSON object")]
    NotAnObject,
    /// The instance's `type` member names no attribute family.
    #[error("the `type` member names no attribute family")]
    NoFamily,
    /// A member the entity or the instance's family requires is absent.
    #[error("missing field `{member}`")]
    Missing {
        /// The member.
        member: RefusedMember,
    },
    /// A member does not decode as what the entity or the family holds in it.
    #[error("invalid `{member}` member: {source}")]
    Member {
        /// The member.
        member: RefusedMember,
        /// The decoding's failure.
        #[source]
        source: serde_json::Error,
    },
    /// A value carrying `type` and `coordinates` is no `GeoJSON` geometry.
    #[error("invalid `{member}` member: {source}")]
    Value {
        /// The member.
        member: RefusedMember,
        /// What is wrong with the geometry.
        #[source]
        source: GeometryError,
    },
    /// A `GeoProperty` geometry is not read as the typed geometry.
    #[error("invalid `{member}` member: {source}")]
    Geometry {
        /// The member.
        member: RefusedMember,
        /// Why the geometry is not read as typed.
        #[source]
        source: TypedGeometryRefusal,
    },
    /// A `JsonProperty` value is no JSON value the broker admits.
    #[error("invalid `{member}` member: {source}")]
    Literal {
        /// The member.
        member: RefusedMember,
        /// Why the JSON value is not admitted.
        #[source]
        source: JsonLiteralError,
    },
    /// A list member is no JSON array.
    #[error("the `{member}` member is no array")]
    NotAnArray {
        /// The member.
        member: RefusedMember,
    },
    /// A member name is no attribute name (clause 4.6.2).
    #[error("invalid attribute name: {0}")]
    AttributeName(#[source] NgsiLdReferenceError),
}

/// Why a compact node gives no typed value.
#[derive(Debug, Error)]
pub enum NodeReadError<U: Error + 'static> {
    /// The node is refused as what it was read as.
    #[error(transparent)]
    Refused(#[from] NodeRefusal),
    /// A node of the tree has no JSON form, see
    /// [`CompactNode::Unreadable`](crate::compact_node::CompactNode::Unreadable).
    #[error(transparent)]
    Unreadable(U),
}
