//! Why the content of a submission cannot be stored as written.
//!
//! A refusal is the client's to correct, as opposed to a fault of the broker:
//! clause 5.5.2 answers it with `BadRequestData` rather than `InternalError`.
//! The reasons live on the commit port because every layer that refuses content
//! meets here: temporal policy interpreting a write's history, the current-state
//! store and the history store all hand their refusal through one atomic commit,
//! and entity and temporal policy both read it back out. Each `#[error]` renders
//! lowercase client prose; spec attribute-type names keep their fixed casing.

use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_date_time::NgsiLdDateTimeError, time_value::TimeValueError};
use andromeda_ngsi_ld_number::JsonNumberError;
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
    /// An `observedAt` value was not a clause 4.6.3 `DateTime`.
    #[error("invalid observedAt: {0}")]
    ObservedAt(#[source] NgsiLdDateTimeError),
    /// A time-value property was not a clause 4.6.3 time value.
    #[error("invalid time value property: {0}")]
    TimeValueProperty(#[source] TimeValueError),
    /// A `DateTime` value cannot be held at the store's microsecond resolution.
    #[error("a DateTime value cannot be stored at microsecond resolution: {0}")]
    TimestampPrecision(#[source] NgsiLdDateTimeError),
    /// A `Relationship` instance omitted its `object`.
    #[error("Relationship missing object")]
    RelationshipMissingObject,
    /// A `GeoProperty` carried a value that is not a geometry.
    #[error("invalid GeoProperty geometry")]
    GeoPropertyGeometryInvalid,
    /// A `VocabProperty` instance omitted its `vocab` value.
    #[error("VocabProperty missing vocab value")]
    VocabPropertyMissingVocab,
    /// A `Property` instance omitted its `value`.
    #[error("Property missing value")]
    PropertyMissingValue,
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
    /// A client-supplied `instanceId` claimed more than one instance of the same
    /// attribute at once (clause 4.5.7).
    #[error("instanceId is already claimed by another instance of the same attribute, and must be unique to one")]
    DuplicateInstanceClaim,
}

#[cfg(test)]
mod tests {
    use crate::submission_refusal::SubmissionRefusal;
    use pretty_assertions::assert_eq;

    #[test]
    fn an_untyped_entity_names_the_entity_the_client_wrote() {
        let entity = "urn:ngsi-ld:Building:1".parse().expect("a valid identifier");
        assert_eq!(
            SubmissionRefusal::UntypedEntity(entity).to_string(),
            "entity urn:ngsi-ld:Building:1 carries no type, but at least one is required"
        );
    }
}
