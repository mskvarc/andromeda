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
    attribute_name::AttributeName,
    identifier::Identifier,
    instance_identity::InstanceIdentity,
    time::{date_time::NgsiLdDateTimeError, value::TimeValueError},
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
    /// A Temporal API write gave one `instanceId` to more than one instance of
    /// one attribute, across its `datasetId`s (clause 4.5.7). Boxed because a
    /// refusal travels inside every temporal error.
    #[error("{0}")]
    RepeatedInstanceId(Box<RepeatedInstanceId>),
}

/// One `instanceId` a write gave to more than one instance of one attribute.
#[derive(Debug, Display)]
#[display("instanceId {} names more than one instance of attribute {attribute}, but must name one", instance.rendered())]
pub struct RepeatedInstanceId {
    /// The attribute whose instances share the identifier.
    pub attribute: AttributeName,
    /// The identifier given more than once.
    pub instance: InstanceIdentity,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::submission_refusal::{RepeatedInstanceId, SubmissionRefusal};
    use andromeda_ngsi_ld_lexicon::instance_identity::InstanceIdentity;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_repeated_instance_id_names_the_identifier_and_its_attribute() {
        let refusal = SubmissionRefusal::RepeatedInstanceId(Box::new(RepeatedInstanceId {
            attribute: "https://example.com/speed".parse().expect("a valid attribute name"),
            instance: InstanceIdentity::Verbatim("urn:ngsi-ld:Instance:x".parse().expect("a valid identifier")),
        }));
        assert_eq!(
            refusal.to_string(),
            "instanceId urn:ngsi-ld:Instance:x names more than one instance of attribute https://example.com/speed, but must name one"
        );
    }

    #[test]
    fn an_untyped_entity_names_the_entity_the_client_wrote() {
        let entity = "urn:ngsi-ld:Building:1".parse().expect("a valid identifier");
        assert_eq!(
            SubmissionRefusal::UntypedEntity(entity).to_string(),
            "entity urn:ngsi-ld:Building:1 carries no type, but at least one is required"
        );
    }
}
