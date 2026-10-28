//! Why an expanded document is not an entity, or not a write a given operation
//! admits.
//!
//! One enum serves the canonical decoder, the conversion of a written document
//! into state and the write admission, because they refuse the same things for
//! the same reasons: clause 5.5.4 makes each of them a `BadRequestData`. No
//! message echoes the raw input, only typed values the broker already validated.

use crate::{
    attribute_instances::RepeatedInstance,
    expanded_value_name::ExpandedValueNameError,
    member_shape::MemberShape,
    refusal_wording::{attribute_prefix, compact_term, forbidden_member_subject},
    scope_refusal::ScopeRefusal,
    syntax_category::SyntaxCategory,
};
use andromeda_ngsi_ld_geometry::geojson::GeometryError;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    datatype::recognised::DatatypeError,
    entity_type_name::EntityTypeName,
    identifier::{Identifier, IdentifierParseError},
    reference::NgsiLdReferenceError,
    unit_code_resolution::UnitCodeRefusal,
};
use iri_rs::Iri;
use thiserror::Error;

/// Why a document is not an entity, or not one the operation admits.
#[derive(Debug, Error)]
pub enum EntityRefusal {
    /// The text is not well-formed JSON. Only the position is reported.
    #[error("the document is not well-formed JSON ({category} error at line {line}, column {column})")]
    Syntax {
        /// What kind of failure the reader met.
        category: SyntaxCategory,
        /// The line of the failure.
        line: usize,
        /// The column of the failure.
        column: usize,
    },
    /// A member is not shaped the way the expanded form shapes it.
    #[error("{}the member '{member}' is not shaped as an expanded NGSI-LD document shapes it", attribute_prefix(.attribute.as_ref()))]
    Malformed {
        /// The attribute the member belongs to, when it belongs to one.
        attribute: Option<AttributeName>,
        /// The member.
        member: MemberShape,
    },
    /// The expanded entity carries no `@id`.
    #[error("the expanded entity carries no '@id'")]
    MissingId,
    /// The `@id` is not an identifier (clause 5.5.4).
    #[error("the entity identifier is not a valid identifier: {source}")]
    EntityId {
        /// Why it is not one.
        source: IdentifierParseError,
    },
    /// An `@type` entry is not a string.
    #[error("an expanded entity '@type' entry is not a string")]
    TypeNotString,
    /// An `@type` entry is not an entity type name.
    #[error("an entity type is not a valid type name: {source}")]
    EntityType {
        /// Why it is not one.
        source: NgsiLdReferenceError,
    },
    /// The entity carries no type (clause 4.5.1).
    #[error("the entity carries no type")]
    MissingType,
    /// A member key is not an attribute name.
    #[error("invalid expanded attribute name: {0}")]
    ExpandedAttributeName(#[source] NgsiLdReferenceError),
    /// A member key is no name expansion produces.
    #[error("invalid expanded member name: {0}")]
    ExpandedMemberName(#[source] ExpandedValueNameError),
    /// The `scope` member is not a value clause 4.18 admits.
    #[error(transparent)]
    Scope(ScopeRefusal),
    /// The `scope` member carries the NGSI-LD Null outside a partial-update or
    /// merge fragment (clause 5.5.4).
    #[error(
        "the scope member carries the NGSI-LD Null, which is admitted as a first-level member value only in the fragment of a partial update or merge operation"
    )]
    ScopeNull,
    /// The entity-level `expiresAt` is not a date-time, or is the NGSI-LD
    /// Null outside a fragment.
    #[error("the entity 'expiresAt' is not an RFC 3339 date-time")]
    EntityExpiresAt,
    /// An attribute instance carries a `datasetId` that is not an identifier
    /// (clauses 4.5.5 and 5.5.4).
    #[error("attribute '{attribute}': the datasetId is not a valid identifier: {source}")]
    DatasetId {
        /// The attribute the instance belongs to.
        attribute: AttributeName,
        /// Why it is not an identifier.
        #[source]
        source: IdentifierParseError,
    },
    /// An attribute instance carries an `instanceId` that is not an identifier
    /// (clauses 4.5.7 and 5.5.4).
    #[error("attribute '{attribute}' carries an instanceId that is not a valid identifier: {source}")]
    InstanceId {
        /// The attribute the instance belongs to.
        attribute: AttributeName,
        /// Why it is not an identifier.
        #[source]
        source: IdentifierParseError,
    },
    /// A temporal member of an instance is not a clause 4.6.3 `DateTime`.
    #[error("attribute '{attribute}': the member '{member}' is not an NGSI-LD DateTime")]
    DateTime {
        /// The attribute the instance belongs to.
        attribute: AttributeName,
        /// The member.
        member: MemberShape,
    },
    /// A literal, at any depth of an instance, is not a value of the datatype
    /// it is declared as, or a `valueType` declares no datatype it may.
    #[error("attribute '{attribute}': {source}")]
    Datatype {
        /// The attribute or sub-attribute the value belongs to.
        attribute: AttributeName,
        /// Why the literal or the `valueType` is refused.
        #[source]
        source: DatatypeError,
    },
    /// A `valueType`, `objectType` or `vocab` entry is not a name.
    #[error("attribute '{attribute}': the member '{member}' is not a valid name: {source}")]
    Reference {
        /// The attribute the instance belongs to.
        attribute: AttributeName,
        /// The member.
        member: MemberShape,
        /// Why it is not a name.
        #[source]
        source: NgsiLdReferenceError,
    },
    /// An attribute set its `datasetId` to the NGSI-LD Null (clauses 5.5.8, 5.5.12).
    #[error("attribute '{attribute}' sets its datasetId to 'urn:ngsi-ld:null'; a datasetId cannot be deleted by setting it to that value")]
    DatasetIdSentinel {
        /// The offending attribute.
        attribute: AttributeName,
    },
    /// An attribute carried the NGSI-LD Null where this operation forbids it (clause 5.5.4).
    #[error("attribute '{attribute}' carries the reserved value 'urn:ngsi-ld:null', which this operation does not permit")]
    ReservedNullValue {
        /// The offending attribute.
        attribute: AttributeName,
    },
    /// An attribute nested the NGSI-LD Null inside its value object outside a
    /// merge fragment (clause 5.5.4).
    #[error(
        "attribute '{attribute}' carries 'urn:ngsi-ld:null' inside the JSON object on the right-hand side of its value, which only a merge fragment may do"
    )]
    NestedNullValue {
        /// The offending attribute.
        attribute: AttributeName,
    },
    /// An attribute instance carried a member its family never defines
    /// (clauses 4.5.2 through 4.5.24).
    #[error("{} cannot carry '{}': that member is never present on this attribute family", forbidden_member_subject(.family.as_ref(), .attribute.as_ref()), compact_term(.member))]
    MemberForbidden {
        /// The attribute the instance belongs to, when it has a name.
        attribute: Option<AttributeName>,
        /// The attribute family, when the instance declares one.
        family: Option<AttributeType>,
        /// The forbidden member, by its expanded IRI.
        member: AttributeName,
    },
    /// An attribute instance declared a `@type` outside the NGSI-LD set (clause 5.2.4).
    #[error("attribute type is not a supported NGSI-LD attribute type")]
    UnsupportedAttributeType,
    /// An attribute instance declared two different families.
    #[error("attribute '{attribute}' declares more than one attribute type")]
    SeveralFamilies {
        /// The offending attribute.
        attribute: AttributeName,
    },
    /// A `Property` or `GeoProperty` instance omitted its required `value` member.
    #[error("{attribute_type} '{attribute}' is missing required field 'value'")]
    MissingValue {
        /// The declared attribute family.
        attribute_type: AttributeType,
        /// The offending attribute.
        attribute: AttributeName,
    },
    /// A `Relationship` instance omitted its required `object` member.
    #[error("Relationship '{attribute}' is missing required field 'object'")]
    MissingObject {
        /// The offending attribute.
        attribute: AttributeName,
    },
    /// An instance of one of the five other families omitted the value member
    /// its family requires (clauses 4.5.18, 4.5.20 to 4.5.24).
    #[error("{family} '{attribute}' is missing its value member")]
    MissingFamilyValue {
        /// The declared attribute family.
        family: AttributeType,
        /// The offending attribute.
        attribute: AttributeName,
    },
    /// An instance of a unitless attribute family carried `unitCode` (clauses
    /// 4.5.3, 4.5.18, 4.5.20, 4.5.24).
    #[error("{family} '{attribute}' cannot carry 'unitCode': this attribute family is defined without one")]
    UnitCodeForbidden {
        /// The unitless attribute family.
        family: AttributeType,
        /// The offending attribute.
        attribute: AttributeName,
    },
    /// The `json` of a `JsonProperty` is neither a JSON object nor an array of
    /// JSON objects (clause 4.5.24.2: "a raw JSON object (or array of
    /// objects)").
    #[error("the 'json' of JsonProperty '{}' must be a JSON object or an array of JSON objects", compact_term(.attribute))]
    JsonNotAnObject {
        /// The offending attribute.
        attribute: AttributeName,
    },
    /// A `unitCode` names no UN/CEFACT common code, or is not one plain
    /// string (Table 5.2.5-1, clause 4.5.2.2).
    #[error("'{}' carries an unusable 'unitCode': {source}", compact_term(.attribute))]
    UnitCodeUnusable {
        /// The offending attribute.
        attribute: AttributeName,
        /// Why the `unitCode` is not usable.
        #[source]
        source: UnitCodeRefusal,
    },
    /// Two instances of one attribute share a `datasetId` (clause 4.5.5).
    #[error("attribute '{attribute}' repeats the instance with datasetId '{dataset_id}'")]
    RepeatedInstance {
        /// The offending attribute.
        attribute: AttributeName,
        /// The repeated dataset identifier.
        dataset_id: Identifier,
    },
    /// An attribute carried more than one default (no-`datasetId`) instance
    /// (clause 4.5.5).
    #[error("attribute '{attribute}' carries more than one default instance")]
    MultipleDefaultInstances {
        /// The offending attribute.
        attribute: AttributeName,
    },
    /// A `GeoProperty` instance did not carry a usable `GeoJSON` geometry (clause 5.5.4).
    #[error("GeoProperty '{}': {source}", compact_term(.attribute))]
    GeoProperty {
        /// The `GeoProperty`, by its expanded IRI.
        attribute: AttributeName,
        /// The geometry fault.
        #[source]
        source: GeometryError,
    },
    /// The NGSI-LD Null stands beside data in one member, where it can neither
    /// delete the member nor be data.
    #[error("{}the NGSI-LD Null stands beside other values in one member, where it neither deletes the member nor is data", attribute_prefix(.attribute.as_ref()))]
    NullBesideData {
        /// The offending attribute, when an attribute holds the member.
        attribute: Option<AttributeName>,
    },
    /// The document holds the NGSI-LD Null where entity state cannot hold it.
    #[error("{}the NGSI-LD Null is a deletion instruction and cannot be part of an entity", attribute_prefix(.attribute.as_ref()))]
    NullInState {
        /// The attribute holding it, when an attribute does.
        attribute: Option<AttributeName>,
    },
    /// A member of an instance that NGSI-LD reserves for responses or for the
    /// Temporal API stands in a stored or written attribute instance.
    #[error("attribute '{attribute}' carries the member '{}', which is never part of a stored attribute instance", compact_term(.member))]
    OutputOnlyMember {
        /// The offending attribute.
        attribute: AttributeName,
        /// The member, by its expanded IRI.
        member: AttributeName,
    },
    /// An attribute is named by a member NGSI-LD defines for itself. Clause
    /// 4.5.1: "Terms defined in the Core Context as non-reified Properties
    /// (such as "datasetId", "instanceId", etc.) shall not be used as Attribute
    /// names"; clause 5.5.7 keeps the core @context from being overridden, so
    /// such a term always expands to the member and never to an attribute.
    #[error(
        "'{}' ({attribute}) is a member the NGSI-LD core @context defines for itself and cannot be used as an attribute name",
        compact_term(.attribute)
    )]
    ReservedAttributeName {
        /// The reserved name, by its expanded IRI.
        attribute: AttributeName,
    },
    /// An attribute name the request's context reads as a compact IRI breaks
    /// the clause 4.6.2 grammar after its colon.
    #[error("'{attribute}' is not an attribute name: the part after the prefix must start with a letter and hold only letters, digits and '_'")]
    PrefixedAttributeName {
        /// The name, as the request wrote it.
        attribute: AttributeName,
    },
    /// An entity type the request's context reads as a compact IRI breaks the
    /// clause 4.6.2 grammar after its colon.
    #[error("'{entity_type}' is not an entity type name: the part after the prefix must start with a letter and hold only letters, digits and '_'")]
    PrefixedEntityType {
        /// The type, as the request wrote it.
        entity_type: EntityTypeName,
    },
}

impl EntityRefusal {
    /// The refusal of an instance of `family` that carries no value under the
    /// member its family reads its value from (clauses 4.5.2, 4.5.3, 4.5.18,
    /// 4.5.20 to 4.5.24, 5.2.5).
    #[must_use]
    pub fn missing_value(attribute: &AttributeName, family: AttributeType) -> EntityRefusal {
        let attribute = attribute.clone();
        match family {
            AttributeType::Property | AttributeType::GeoProperty => EntityRefusal::MissingValue {
                attribute_type: family,
                attribute,
            },
            AttributeType::Relationship => EntityRefusal::MissingObject { attribute },
            AttributeType::LanguageProperty
            | AttributeType::VocabProperty
            | AttributeType::JsonProperty
            | AttributeType::ListProperty
            | AttributeType::ListRelationship => EntityRefusal::MissingFamilyValue { family, attribute },
        }
    }

    /// The refusal of two instances of `attribute` naming one instance
    /// (clause 4.5.5.1).
    #[must_use]
    pub fn repeated_instance(attribute: &AttributeName, repeated: RepeatedInstance) -> EntityRefusal {
        // clone: the refusal outlives the borrowed attribute name.
        let attribute = attribute.clone();
        match repeated {
            RepeatedInstance::Default => EntityRefusal::MultipleDefaultInstances { attribute },
            RepeatedInstance::Dataset(dataset_id) => EntityRefusal::RepeatedInstance { attribute, dataset_id },
        }
    }

    /// The refusal of the member `member` on an instance whose family never
    /// defines it (clauses 4.5.2.2 to 4.5.24.2).
    #[must_use]
    pub fn member_forbidden(attribute: Option<&AttributeName>, family: Option<AttributeType>, member: Iri<&'static str>) -> EntityRefusal {
        match member.as_str().parse::<AttributeName>() {
            Ok(member) => EntityRefusal::MemberForbidden {
                attribute: attribute.cloned(),
                family,
                member,
            },
            Err(source) => EntityRefusal::ExpandedAttributeName(source),
        }
    }

    /// The refusal for a JSON text the reader could not read.
    #[must_use]
    pub fn syntax(error: &serde_json::Error) -> EntityRefusal {
        EntityRefusal::Syntax {
            category: SyntaxCategory::from(error.classify()),
            line: error.line(),
            column: error.column(),
        }
    }
}
