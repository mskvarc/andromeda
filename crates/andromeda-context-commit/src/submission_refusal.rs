//! Why the content of a submission cannot be stored as written.
//!
//! A refusal is the client's to correct, as opposed to a fault of the broker:
//! clause 5.5.2 answers it with `BadRequestData` rather than `InternalError`.
//! The reasons live on the commit port because every layer that refuses content
//! meets here: temporal policy interpreting a write's history, the current-state
//! store and the history store all hand their refusal through one atomic commit,
//! and entity and temporal policy both read it back out. Each `#[error]` renders
//! lowercase client prose; spec attribute-type names keep their fixed casing.

use andromeda_ngsi_ld_lexicon::{
    identifier::{Identifier, IdentifierParseError},
    ngsi_ld_date_time::NgsiLdDateTimeError,
    reference::NgsiLdReferenceError,
    time_value::TimeValueError,
    uri::UriParseError,
};
use andromeda_ngsi_ld_number::JsonNumberError;
use derive_more::Display;
use thiserror::Error;

/// Why the submitted content cannot be stored as written.
#[derive(Debug, Error)]
pub enum SubmissionRefusal {
    /// An Entity carried no type; clause 4.5.1 requires at least one.
    #[error("entity {0} carries no type, but at least one is required")]
    UntypedEntity(Identifier),
    /// An Entity carried no type, and the store that refused it names no row.
    #[error("an entity write carried no type, but at least one is required")]
    UntypedEntityUnidentified,
    /// A `unitCode` member was not one plain string (clause 4.5.2.2).
    #[error("'unitCode' shall be a single string holding a UN/CEFACT common code, not a reified attribute or a repeated member")]
    UnitCodeMalformed,
    /// A `unitCode` names no code the closed UN/CEFACT list holds (clause 4.5.2).
    #[error("'unitCode' is not a UN/CEFACT common code the unit catalog registers")]
    UnitCodeUnregistered,
    /// An expanded attribute member key did not satisfy the attribute-name grammar.
    #[error("expanded attribute name is invalid: {0}")]
    ExpandedAttributeName(#[source] NgsiLdReferenceError),
    /// An attribute's `datasetId` was not an identifier.
    #[error("invalid datasetId: {0}")]
    DatasetId(#[source] IdentifierParseError),
    /// An `observedAt` value was not a clause 4.6.3 `DateTime`.
    #[error("invalid observedAt: {0}")]
    ObservedAt(#[source] NgsiLdDateTimeError),
    /// An `instanceId` value was not an identifier.
    #[error("invalid instanceId: {0}")]
    InstanceId(#[source] IdentifierParseError),
    /// A time-value property was not a clause 4.6.3 time value.
    #[error("invalid time value property: {0}")]
    TimeValueProperty(#[source] TimeValueError),
    /// A `DateTime` value cannot be held at the store's microsecond resolution.
    #[error("a DateTime value cannot be stored at microsecond resolution: {0}")]
    TimestampPrecision(#[source] NgsiLdDateTimeError),
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
        member: RelationshipTargetMember,
        /// The `URI` grammar violation.
        #[source]
        source: UriParseError,
    },
    /// A `GeoProperty` instance omitted its `value`.
    #[error("GeoProperty missing value")]
    GeoPropertyMissingValue,
    /// A `GeoProperty` carried a `value` string that was not `GeoJSON`.
    #[error("invalid GeoProperty GeoJSON string: {0}")]
    GeoPropertyGeoJson(#[source] serde_json::Error),
    /// A `GeoProperty` carried a value that is not a geometry.
    #[error("invalid GeoProperty geometry")]
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
    #[error("Property number cannot be carried: {0}")]
    PropertyNumber(#[source] JsonNumberError),
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
    /// A `ListRelationship` `objectList` was not a list.
    #[error("ListRelationship objectList is not a list")]
    ListRelationshipObjectListNotAList,
    /// A `ListRelationship` `objectList` member named no target `URI`.
    #[error("ListRelationship objectList member names no target URI")]
    ListRelationshipObjectListMemberNoTarget,
    /// A client-supplied `instanceId` claimed more than one instance of the same
    /// attribute at once (clause 4.5.7).
    #[error("instanceId is already claimed by another instance of the same attribute, and must be unique to one")]
    DuplicateInstanceClaim,
}

/// The member a Relationship target was read from, named as the client wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum RelationshipTargetMember {
    /// The `object` of a `Relationship`.
    #[display("Relationship object")]
    RelationshipObject,
    /// One member of the `objectList` of a `ListRelationship`.
    #[display("ListRelationship objectList member")]
    ListRelationshipObjectListMember,
}

#[cfg(test)]
mod tests {
    use crate::submission_refusal::{RelationshipTargetMember, SubmissionRefusal};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use pretty_assertions::assert_eq;

    #[test]
    fn an_untyped_entity_names_the_entity_the_client_wrote() {
        let entity = "urn:ngsi-ld:Building:1".parse().expect("a valid identifier");
        assert_eq!(
            SubmissionRefusal::UntypedEntity(entity).to_string(),
            "entity urn:ngsi-ld:Building:1 carries no type, but at least one is required"
        );
    }

    #[test]
    fn an_unregistered_unit_code_does_not_echo_the_code() {
        assert_eq!(
            SubmissionRefusal::UnitCodeUnregistered.to_string(),
            "'unitCode' is not a UN/CEFACT common code the unit catalog registers"
        );
    }

    #[test]
    fn a_relationship_target_refusal_names_the_member_it_came_from() {
        let source = "not a uri".parse::<Uri>().expect_err("a space is not allowed in a URI");
        let refusal = SubmissionRefusal::RelationshipTarget {
            member: RelationshipTargetMember::ListRelationshipObjectListMember,
            source,
        };
        assert!(
            refusal.to_string().starts_with("ListRelationship objectList member is not a valid URI: "),
            "{refusal}"
        );
    }
}
