//! The concrete reasons a temporal request or representation is rejected as
//! `BadRequestData`.
//!
//! Every client-facing validation failure of temporal policy is one variant
//! here, carrying the offending value as a typed field and chaining the typed
//! source error where one exists, so
//! [`TemporalError::Validation`](crate::error::TemporalError::Validation) never
//! holds an ad-hoc string. Each `#[error]` renders lowercase client prose; spec
//! attribute-type names (`Property`, `GeoProperty`, `Relationship`, ...) keep
//! their fixed casing, and clause numbers stay in doc comments.

use andromeda_ngsi_ld_lexicon::{
    attribute_type::AttributeType,
    identifier::IdentifierParseError,
    ngsi_ld_date_time::NgsiLdDateTimeError,
    reference::NgsiLdReferenceError,
    time_value::TimeValueError,
    uri::UriParseError,
};
use andromeda_ngsi_ld_number::JsonNumberError;
use thiserror::Error;

/// Why a temporal request or representation is invalid.
#[derive(Debug, Error)]
pub enum TemporalValidationReason {
    /// The temporal entity payload was not a JSON object.
    #[error("entity must be a JSON object")]
    EntityNotObject,
    /// The temporal entity payload omitted its `id`.
    #[error("entity is missing an id")]
    EntityMissingId,
    /// The temporal entity payload omitted its `type`.
    #[error("entity is missing a type")]
    EntityMissingType,
    /// The temporal entity's `id` was not an identifier.
    #[error("invalid entity id: {source}")]
    EntityId {
        /// Why it is not an identifier, a length past the identifier bound included.
        #[source]
        source: IdentifierParseError,
    },
    /// An attribute name did not satisfy the attribute-name grammar.
    #[error("attribute name is invalid: {source}")]
    AttributeName {
        /// The grammar violation.
        #[source]
        source: NgsiLdReferenceError,
    },
    /// An expanded attribute member key did not satisfy the attribute-name grammar.
    #[error("invalid expanded attribute name '{name}': {source}")]
    ExpandedAttributeName {
        /// The rejected expanded member key.
        name: String,
        /// The grammar violation.
        #[source]
        source: NgsiLdReferenceError,
    },
    /// An attribute's `datasetId` was not an identifier.
    #[error("invalid datasetId '{raw}': {source}")]
    DatasetId {
        /// The rejected raw value.
        raw: String,
        /// Why it is not an identifier, a length past the identifier bound included.
        #[source]
        source: IdentifierParseError,
    },
    /// An `observedAt` value was not a clause 4.6.3 `DateTime`.
    #[error("invalid observedAt '{raw}': {source}")]
    ObservedAt {
        /// The rejected raw value.
        raw: String,
        /// The `DateTime` grammar violation.
        #[source]
        source: NgsiLdDateTimeError,
    },
    /// An `instanceId` value was not an identifier.
    #[error("invalid instanceId '{raw}': {source}")]
    InstanceId {
        /// The rejected raw value.
        raw: String,
        /// Why it is not an identifier, a length past the identifier bound included.
        #[source]
        source: IdentifierParseError,
    },
    /// A time-value property was not a clause 4.6.3 time value.
    #[error("invalid time value property '{raw}': {source}")]
    TimeValueProperty {
        /// The rejected raw value.
        raw: String,
        /// The time-value grammar violation.
        #[source]
        source: TimeValueError,
    },
    /// A `Relationship` instance omitted its `object`.
    #[error("Relationship missing object")]
    RelationshipMissingObject,
    /// A `Relationship` `object` named no target `URI`.
    #[error("Relationship object names no target URI")]
    RelationshipObjectNoTarget,
    /// A `Relationship` target member was not a `URI`.
    #[error("{member} is not a valid URI: {source}")]
    RelationshipTarget {
        /// The member the target came from.
        member: String,
        /// The `URI` grammar violation.
        #[source]
        source: UriParseError,
    },
    /// A `GeoProperty` instance omitted its `value`.
    #[error("GeoProperty missing value")]
    GeoPropertyMissingValue,
    /// A `GeoProperty` carried a `value` string that was not `GeoJSON`.
    #[error("invalid GeoProperty GeoJSON string: {source}")]
    GeoPropertyGeoJson {
        /// The JSON parse failure.
        #[source]
        source: serde_json::Error,
    },
    /// A `GeoProperty` carried an expanded node that was not a geometry.
    #[error("invalid expanded GeoProperty geometry")]
    GeoPropertyGeometryInvalid,
    /// A `VocabProperty` instance omitted its `vocab` value.
    #[error("VocabProperty missing vocab value")]
    VocabPropertyMissingVocab,
    /// A `Property` instance omitted its `value`.
    #[error("Property missing value")]
    PropertyMissingValue,
    /// A `Property` array element carried no `value`.
    #[error("Property array element carries no value")]
    PropertyArrayElementMissingValue,
    /// A `Property` carried a number the arbitrary-precision store cannot hold.
    #[error("Property number cannot be carried: {source}")]
    PropertyNumber {
        /// The number-encoding failure.
        #[source]
        source: JsonNumberError,
    },
    /// A `LanguageProperty` instance omitted its `languageMap`.
    #[error("LanguageProperty missing languageMap")]
    LanguagePropertyMissingLanguageMap,
    /// A `JsonProperty` instance omitted its `json` value.
    #[error("JsonProperty missing json value")]
    JsonPropertyMissingJson,
    /// A `ListProperty` instance omitted its `valueList`.
    #[error("ListProperty missing valueList")]
    ListPropertyMissingValueList,
    /// A `ListRelationship` instance omitted its `objectList`.
    #[error("ListRelationship missing objectList")]
    ListRelationshipMissingObjectList,
    /// A `ListRelationship` `objectList` member named no target `URI`.
    #[error("ListRelationship objectList member names no target URI")]
    ListRelationshipObjectListMemberNoTarget,
    /// An instance of a unitless attribute family carried `unitCode` (clauses
    /// 4.5.3, 4.5.18, 4.5.20, 4.5.24).
    #[error("{family} '{attribute}' cannot carry 'unitCode': this attribute family is defined without one")]
    InstanceUnitCodeForbidden {
        /// The unitless attribute family.
        family: AttributeType,
        /// The offending attribute, by its expanded name.
        attribute: String,
    },
    /// The client's `collation` is not a locale the database can honour.
    #[error("'{collation}' does not conform to a valid ICU collation")]
    Collation {
        /// The rejected collation identifier.
        collation: String,
    },
    /// The history store refused the submission as unusable.
    ///
    /// The text is relayed verbatim from the store adapter, which lives in a
    /// crate this one cannot depend on without a dependency cycle, so its reason
    /// arrives as opaque prose rather than a typed cause.
    #[error("{detail}")]
    Store {
        /// The store adapter's own refusal detail.
        detail: String,
    },
    /// The atomic commit port refused the submission as unusable (clause 5.5.6).
    ///
    /// The text is relayed verbatim from the commit port, which cannot depend on
    /// the domain error types without a dependency cycle.
    #[error("{detail}")]
    CommitRefused {
        /// The commit port's own refusal detail.
        detail: String,
    },
}
