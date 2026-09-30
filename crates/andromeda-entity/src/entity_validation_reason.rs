//! The concrete reasons an entity operation is rejected as `BadRequestData`.
//!
//! Every client-facing validation failure of entity policy is one variant here,
//! carrying the offending value as a typed field and chaining the typed source
//! error where one exists, so [`EntityError::Validation`](crate::error::EntityError::Validation)
//! never holds an ad-hoc string. Each `#[error]` renders lowercase client prose;
//! spec clause numbers stay in these doc comments, never in the message.

use crate::merge_fragment::MergeFragmentError;
use andromeda_context_commit::submission_refusal::SubmissionRefusal;
use andromeda_ngsi_ld_geometry::geojson::GeometryError;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    identifier::{Identifier, IdentifierParseError},
    reference::NgsiLdReferenceError,
};
use andromeda_ngsi_ld_model::expanded::{
    dataset_id::InstanceDatasetIdError,
    entity_member::ExpandedMemberError,
    mutation::error::ExpandedEntityError,
    scope_member::ScopeMemberError,
};
use andromeda_ngsi_ld_query::ordering::collation::IcuCollation;
use andromeda_ngsi_ld_vocabulary::expanded_to_compact;
use thiserror::Error;

/// Why an entity payload or operation is invalid.
#[derive(Debug, Error)]
pub enum EntityValidationReason {
    /// An Attribute member name of a compacted entity does not satisfy the
    /// attribute-name grammar.
    #[error("invalid attribute member name: {0}")]
    AttributeMemberName(#[source] NgsiLdReferenceError),
    /// An expanded JSON-LD member key does not satisfy the attribute-name grammar.
    #[error("invalid expanded attribute name: {0}")]
    ExpandedAttributeName(#[source] NgsiLdReferenceError),
    /// An expanded attribute IRI does not satisfy the attribute-name grammar.
    #[error("invalid attribute name: {0}")]
    AttributeName(#[source] NgsiLdReferenceError),
    /// An expanded entity's `@id`, `@type` or member key cannot be read as the
    /// identifier, type name or attribute name it stands for (clause 5.5.4).
    #[error(transparent)]
    ExpandedMember(ExpandedMemberError),
    /// An attribute instance carries a `datasetId` that is not an identifier
    /// (clauses 4.5.5 and 5.5.4).
    #[error("attribute '{attribute}': {source}")]
    ExpandedDatasetId {
        /// The attribute the instance belongs to.
        attribute: AttributeName,
        /// The unreadable `datasetId`.
        #[source]
        source: InstanceDatasetIdError,
    },
    /// An attribute instance carries an `instanceId` that is not an identifier
    /// (clauses 4.5.7 and 5.5.4).
    #[error("attribute '{attribute}' carries an instanceId that is not a valid identifier: {source}")]
    ExpandedInstanceId {
        /// The attribute the instance belongs to.
        attribute: AttributeName,
        /// Why it is not an identifier.
        #[source]
        source: IdentifierParseError,
    },
    /// An attribute name begins with the reserved `@` character.
    #[error("invalid attribute name '{name}': names beginning with '@' are reserved")]
    ReservedAttributeName {
        /// The rejected attribute name.
        name: AttributeName,
    },
    /// The short name the active `@context` declares for a requested attribute is
    /// not itself an NGSI-LD name.
    #[error("the context declares a short name for {requested} that is not an NGSI-LD name: {source}")]
    DeclaredName {
        /// The attribute the client requested.
        requested: AttributeName,
        /// The grammar violation.
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
    /// The `scope` member carried the NGSI-LD Null outside a partial-update or
    /// merge fragment (clause 5.5.4).
    #[error(
        "the scope member carries the NGSI-LD Null, which is admitted as a first-level member value only in the fragment of a partial update or merge operation"
    )]
    ScopeNull,
    /// The `scope` member was not a value the scope grammar admits.
    #[error(transparent)]
    Scope(#[from] ScopeMemberError),
    /// `scope` was named in a partial-update, which cannot target it (clause 5.6.4).
    #[error("scope is not a partially updatable attribute")]
    ScopeNotPartiallyUpdatable,
    /// `scope` was named in an attribute replacement, which cannot target it.
    #[error("scope is not a replaceable attribute")]
    ScopeNotReplaceable,
    /// An attribute instance carried a member its family never defines (clauses
    /// 4.5.2 through 4.5.24).
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
    /// An instance of a unitless attribute family carried `unitCode` (clauses
    /// 4.5.3, 4.5.18, 4.5.20, 4.5.24).
    #[error("{family} '{attribute}' cannot carry 'unitCode': this attribute family is defined without one")]
    UnitCodeForbidden {
        /// The unitless attribute family.
        family: AttributeType,
        /// The offending attribute.
        attribute: AttributeName,
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
    /// A merge fragment could not be adapted to the stored attribute's shape.
    #[error(transparent)]
    MemberAdaptation(#[from] MergeFragmentError),
    /// The entity could not be serialized to a JSON payload.
    #[error("entity payload could not be serialized: {0}")]
    EntityPayload(#[source] serde_json::Error),
    /// The patch could not be serialized to a JSON payload.
    #[error("patch payload could not be serialized: {0}")]
    PatchPayload(#[source] serde_json::Error),
    /// The compacted entity could not be read back as an entity.
    #[error("entity data is not valid: {0}")]
    EntityData(#[source] serde_json::Error),
    /// The entity representation was not a JSON object.
    #[error("entity data is not a JSON object")]
    EntityDataNotObject,
    /// The entity representation omitted its `id`.
    #[error("entity is missing an id")]
    EntityMissingId,
    /// A Replace Entity payload names an entity other than the one it replaces:
    /// clause 5.6.18.4 replaces the target "with the same Entity ID".
    #[error("the replacement names the entity '{named}', not the entity its URL names")]
    ReplacementNamesAnotherEntity {
        /// The entity the payload names.
        named: Identifier,
    },
    /// A distance ordering was requested with nothing to measure from (clause 5.7.2.4).
    #[error("distance-based ordering requires either a geo-query or an orderFrom parameter")]
    DistanceOrderingWithoutReference,
    /// The client's `collation` is not a locale the database can honour.
    #[error("'{collation}' does not conform to a valid ICU collation")]
    Collation {
        /// The rejected collation identifier.
        collation: IcuCollation,
    },
    /// A `noOverwrite` batch update named only attributes that already exist, so
    /// clause 5.6.9.4's local Append (clause 5.6.3) appended nothing and the
    /// Entity is not "successfully updated (Attributes appended)" for S (clause 5.6.9.5).
    #[error("no attributes were appended: every named attribute already exists and noOverwrite was set")]
    NoOverwriteConflict,
    /// The atomic commit port refused the submission as unusable (clause 5.5.6).
    #[error(transparent)]
    CommitRefused(SubmissionRefusal),
    /// An expanded entity- or attribute-level mutation rejected malformed input.
    #[error(transparent)]
    Expanded(ExpandedEntityError),
}

/// Render the subject of a forbidden-member message from its typed parts.
///
/// Named after the attribute where the instance has one, and after its family's
/// fragment otherwise, matching how the specification phrases the prohibition.
fn forbidden_member_subject(family: Option<&AttributeType>, attribute: Option<&AttributeName>) -> String {
    match (family, attribute) {
        (Some(family), Some(attribute)) => format!("{family} '{attribute}'"),
        (Some(family), None) => format!("the {family} fragment"),
        (None, Some(attribute)) => format!("attribute '{attribute}'"),
        (None, None) => "the attribute fragment".to_string(),
    }
}

/// Name an attribute or member by its compact term where the core context
/// defines one, and by its expanded IRI otherwise: a user-defined term's
/// compaction depends on the request's own `@context`, which validation does
/// not hold, and an IRI the client can look up beats a guess it cannot.
fn compact_term(name: &AttributeName) -> &str {
    expanded_to_compact(name.as_str()).unwrap_or_else(|| name.as_str())
}
