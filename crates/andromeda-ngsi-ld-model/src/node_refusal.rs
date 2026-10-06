//! Why a compact node is not read as the typed entity, attribute member or
//! attribute instance it stands for.
//!
//! Every node standing where an attribute stands is read into a typed member
//! the specification defines or refused (clause 5.5.4): a refusal names what is
//! wrong and where, from the top of the document read. A node that has no JSON
//! form is another matter, and is told apart: it is a fault of the tree being
//! read, not of what a client or a peer wrote.

use crate::{
    attribute::geo_property::TypedGeometryRefusal,
    temporal::{aggregated_decoding::AggregateRefusal, plural_member_refusal::PluralMemberRefusal},
};
use andromeda_json_literal::admission::JsonLiteralError;
use andromeda_ngsi_ld_geometry::geojson::GeometryError;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, reference::NgsiLdReferenceError};
use andromeda_ngsi_ld_number::json_value::json_pointer::{JsonPointer, JsonPointerSegment};
use andromeda_ngsi_ld_representation::attribute_type_validation::AttributeNodeFault;
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
    /// The `entityIdSealed` of an `ngsildproof` Property.
    #[display("{}", compact_properties::ENTITY_ID_SEALED)]
    EntityIdSealed,
    /// The `entityTypeSealed` of an `ngsildproof` Property.
    #[display("{}", compact_properties::ENTITY_TYPE_SEALED)]
    EntityTypeSealed,
}

/// What is wrong with a compact node read as an entity, an attribute member
/// or an attribute instance.
#[derive(Debug, Error)]
pub enum NodeRefusal {
    /// The node is no JSON object.
    #[error("an entity or an attribute instance is a JSON object")]
    NotAnObject,
    /// A node standing where an attribute stands is none the clauses admit
    /// (clauses 4.5.2.3, 4.5.5 and 5.2.4).
    #[error("the attribute {0}")]
    Shape(AttributeNodeFault),
    /// A JSON `null` stands where a value is required: no member of an
    /// NGSI-LD element admits a null value directly (clause 5.2.1).
    #[error("a JSON null stands where a value is required")]
    Null,
    /// A member the entity or the instance's family requires is absent.
    #[error("missing field `{member}`")]
    Missing {
        /// The member.
        member: RefusedMember,
    },
    /// A member does not decode as what the entity or the family holds in it.
    #[error("invalid '{member}' member: {source}")]
    Member {
        /// The member.
        member: RefusedMember,
        /// The decoding's failure.
        #[source]
        source: serde_json::Error,
    },
    /// A value carrying `type` and `coordinates` is no `GeoJSON` geometry.
    #[error("invalid '{member}' member: {source}")]
    Value {
        /// The member.
        member: RefusedMember,
        /// What is wrong with the geometry.
        #[source]
        source: GeometryError,
    },
    /// A `GeoProperty` geometry is not read as the typed geometry.
    #[error("invalid '{member}' member: {source}")]
    Geometry {
        /// The member.
        member: RefusedMember,
        /// Why the geometry is not read as typed.
        #[source]
        source: TypedGeometryRefusal,
    },
    /// A value held as JSON is no JSON value the broker admits.
    #[error("invalid '{member}' member: {source}")]
    Literal {
        /// The member.
        member: RefusedMember,
        /// Why the JSON value is not admitted.
        #[source]
        source: JsonLiteralError,
    },
    /// A `unitCode` stands on a family that is unitless (clauses 4.5.3.2,
    /// 4.5.18.2, 4.5.20.2, 4.5.22.2 and 4.5.24.2 prohibit it).
    #[error("attribute '{attribute}': a {family} carries no 'unitCode'")]
    UnitCodeForbidden {
        /// The attribute carrying the unit.
        attribute: AttributeName,
        /// The unitless family.
        family: AttributeType,
    },
    /// A `unitCode` is no UN/CEFACT common code (Table 5.2.5-1).
    #[error("attribute '{attribute}': 'unitCode' is not a UN/CEFACT common code")]
    UnitCodeUnusable {
        /// The attribute carrying the unit.
        attribute: AttributeName,
        /// The decoding's failure.
        #[source]
        source: serde_json::Error,
    },
    /// A list member is no JSON array.
    #[error("the '{member}' member is no array")]
    NotAnArray {
        /// The member.
        member: RefusedMember,
    },
    /// A member name is no attribute name (clause 4.6.2).
    #[error("invalid attribute name: {0}")]
    AttributeName(#[source] NgsiLdReferenceError),
    /// An attribute written in the simplified temporal representation (clause
    /// 4.5.9) is none the representation admits.
    #[error(transparent)]
    PluralMember(#[from] Box<PluralMemberRefusal>),
    /// An attribute written in the aggregated representation (clause 4.5.19)
    /// is none the representation admits.
    #[error(transparent)]
    Aggregate(#[from] AggregateRefusal),
}

/// A refusal and where it stands, from the top of the document read.
#[derive(Debug, Error)]
#[error("the node at {path} is refused: {refusal}")]
pub struct RefusedNode {
    /// Where the refused node stands.
    pub path: JsonPointer,
    /// What is wrong with it.
    #[source]
    pub refusal: NodeRefusal,
}

/// Why a compact node gives no typed value.
#[derive(Debug, Error)]
pub enum NodeReadError<U: Error + 'static> {
    /// The node is refused as what it was read as.
    #[error(transparent)]
    Refused(RefusedNode),
    /// A node of the tree has no JSON form, see
    /// [`CompactNode::Unreadable`](andromeda_ngsi_ld_representation::compact_node::CompactNode::Unreadable).
    #[error(transparent)]
    Unreadable(U),
}

impl<U: Error + 'static> From<NodeRefusal> for NodeReadError<U> {
    fn from(refusal: NodeRefusal) -> NodeReadError<U> {
        NodeReadError::Refused(RefusedNode {
            path: JsonPointer::from_segments(Vec::new()),
            refusal,
        })
    }
}

impl<U: Error + 'static> NodeReadError<U> {
    /// The same failure, read from one step further up the document: a
    /// refusal found inside the member or element `segment` names is located
    /// below it.
    #[must_use]
    pub fn within(self, segment: JsonPointerSegment) -> NodeReadError<U> {
        match self {
            NodeReadError::Refused(RefusedNode { path, refusal }) => NodeReadError::Refused(RefusedNode {
                path: JsonPointer::from_segments([segment].into_iter().chain(path.segments().iter().cloned()).collect()),
                refusal,
            }),
            NodeReadError::Unreadable(unreadable) => NodeReadError::Unreadable(unreadable),
        }
    }

    /// The same failure, read from inside the member `name`.
    #[must_use]
    pub fn within_member(self, name: &str) -> NodeReadError<U> {
        self.within(JsonPointerSegment::Member(Box::from(name)))
    }

    /// The same failure, read from inside the element `index`.
    #[must_use]
    pub fn within_element(self, index: usize) -> NodeReadError<U> {
        self.within(JsonPointerSegment::Index(index))
    }
}
