//! What one instance a written Temporal Evolution states records: an
//! observation, or the deletion of the instance.
//!
//! Clauses 4.5.7 and 4.5.8 record a deleted Property or Relationship as an
//! instance whose value member is the NGSI-LD Null beside `deletedAt`, and
//! clause 5.5.4 excepts that representation in the temporal evolution from the
//! Null prohibition, so a Temporal Evolution a client writes (clauses 5.6.11
//! and 5.6.12) may state deletions among its instances. Each one is an instance
//! of the evolution, placed at the `observedAt` it declares. Its `deletedAt` is
//! output-only (Table 5.2.5-2) and system generated (clause 4.8), so the
//! deletion is what the Null states, and the instant it is recorded at is the
//! broker's, as `createdAt` and `modifiedAt` are.

use crate::{
    error::TemporalError,
    observed_instance::{ObservedInstance, declared_observed_at},
};
use andromeda_ngsi_ld_entity::document::{
    instance_document::{InstanceDocument, InstanceDocumentParts},
    written_value::ValueSlot,
};
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, identifier::Identifier};
use chrono::{DateTime, Utc};

/// What one written instance records.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum WrittenInstance {
    /// An observation of a value.
    Observation(ObservedInstance),
    /// The deletion of the instance.
    Deletion(DeletedInstance),
}

/// One written instance stating the deletion of the instance it names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeletedInstance {
    /// The family the deleted instance belongs to, which the deletion
    /// renders the Null member of (clauses 4.5.7 and 4.5.8).
    pub(crate) attribute_type: AttributeType,
    /// The instance's `datasetId`, absent for the default instance.
    pub(crate) dataset_id: Option<Identifier>,
    /// The `observedAt` the deletion declares.
    pub(crate) observed_at: Option<DateTime<Utc>>,
    /// The `instanceId` the client gave the deletion (clause 4.5.7).
    pub(crate) instance_id: Option<Identifier>,
}

impl WrittenInstance {
    /// Read one written instance, `None` when it records nothing: an instance
    /// that names neither a family nor a value observes nothing.
    ///
    /// An instance whose value member is its family's deletion encoding
    /// (clause 4.5.0) is a deletion, of the family it declares or else the one
    /// its value member implies; every other instance is read by
    /// [`ObservedInstance::read`].
    ///
    /// # Errors
    ///
    /// Returns the errors of [`ObservedInstance::read`] for an observation,
    /// and [`TemporalError::Validation`] when a deletion declares an
    /// `observedAt` that is not a clause 4.6.3 `DateTime` in UTC.
    pub(crate) fn read(instance: InstanceDocument) -> Result<Option<WrittenInstance>, TemporalError> {
        let ValueSlot::Remove(carrier) = *instance.value() else {
            return Ok(ObservedInstance::read(instance)?.map(WrittenInstance::Observation));
        };
        let InstanceDocumentParts {
            family,
            dataset_id,
            observed_at,
            instance_id,
            ..
        } = instance.into_parts();
        Ok(Some(WrittenInstance::Deletion(DeletedInstance {
            attribute_type: family.unwrap_or_else(|| carrier.implied_family()),
            dataset_id,
            observed_at: declared_observed_at(observed_at.as_ref()),
            instance_id,
        })))
    }
}

#[cfg(test)]
mod tests {
    use crate::written_instance::{DeletedInstance, WrittenInstance};
    use andromeda_ngsi_ld_entity::document::instance_document::InstanceDocument;
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, time::date_time::NgsiLdDateTime};
    use andromeda_ngsi_ld_model::temporal::TemporalValue;
    use andromeda_ngsi_ld_vocabulary::expanded::{classes as expanded_classes, properties as expanded_properties};
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};

    const PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/Property";
    const RELATIONSHIP: &str = "https://uri.etsi.org/ngsi-ld/Relationship";

    /// What the expanded instance `instance` of `speed` records.
    fn read(instance: &Value) -> Option<WrittenInstance> {
        let attribute: AttributeName = "https://example.org/speed".parse().expect("a valid attribute name");
        let document = InstanceDocument::from_slice(&attribute, &serde_json::to_vec(instance).expect("JSON")).expect("an expanded instance");
        WrittenInstance::read(document).expect("a readable instance")
    }

    fn at(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("a valid instant").get()
    }

    fn date_time(text: &str) -> Value {
        json!([{"@value": text, "@type": expanded_classes::DATE_TIME.as_str()}])
    }

    /// Clauses 4.5.7 and 5.5.4: a Property instance whose value is the Null
    /// and which carries `deletedAt` is a deletion, placed at its
    /// `observedAt` and keeping its `datasetId` and `instanceId`.
    #[test]
    fn a_null_valued_property_with_deleted_at_is_a_deletion_at_its_observed_at() {
        let instance = json!({
            "@type": [PROPERTY],
            expanded_properties::VALUE.as_str(): [{"@value": "urn:ngsi-ld:null"}],
            expanded_properties::OBSERVED_AT.as_str(): date_time("2024-01-02T00:00:00Z"),
            expanded_properties::DELETED_AT.as_str(): date_time("2024-01-02T00:00:00Z"),
            expanded_properties::DATASET_ID.as_str(): [{"@id": "urn:ngsi-ld:Dataset:a"}],
            expanded_properties::INSTANCE_ID.as_str(): [{"@id": "urn:ngsi-ld:instance:1"}]
        });
        pretty_assertions::assert_eq!(
            read(&instance),
            Some(WrittenInstance::Deletion(DeletedInstance {
                attribute_type: AttributeType::Property,
                dataset_id: Some("urn:ngsi-ld:Dataset:a".parse().expect("a valid identifier")),
                observed_at: Some(at("2024-01-02T00:00:00Z")),
                instance_id: Some("urn:ngsi-ld:instance:1".parse().expect("a valid identifier")),
            }))
        );
    }

    /// Clause 4.5.8: a Relationship whose object is the Null is a deletion of
    /// a Relationship; one declaring no `observedAt` is placed at none.
    #[test]
    fn a_null_object_is_the_deletion_of_a_relationship() {
        let instance = json!({
            "@type": [RELATIONSHIP],
            expanded_properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:null"}],
            expanded_properties::DELETED_AT.as_str(): date_time("2024-01-02T00:00:00Z")
        });
        pretty_assertions::assert_eq!(
            read(&instance),
            Some(WrittenInstance::Deletion(DeletedInstance {
                attribute_type: AttributeType::Relationship,
                dataset_id: None,
                observed_at: None,
                instance_id: None,
            }))
        );
    }

    /// An instance naming no family is a deletion of the family its value
    /// member implies (clause 4.5.2 reads `value` as a Property's).
    #[test]
    fn a_deletion_naming_no_family_is_of_the_family_its_value_member_implies() {
        let instance = json!({expanded_properties::VALUE.as_str(): [{"@value": "urn:ngsi-ld:null"}]});
        let Some(WrittenInstance::Deletion(deletion)) = read(&instance) else {
            panic!("a deletion");
        };
        pretty_assertions::assert_eq!(deletion.attribute_type, AttributeType::Property);
    }

    /// An instance holding a value is the observation it records.
    #[test]
    fn an_instance_holding_a_value_is_an_observation() {
        let instance = json!({"@type": [PROPERTY], expanded_properties::VALUE.as_str(): [{"@value": 1}]});
        let Some(WrittenInstance::Observation(observed)) = read(&instance) else {
            panic!("an observation");
        };
        pretty_assertions::assert_eq!(observed.value, TemporalValue::Number("1".parse().expect("a number")));
    }

    /// An instance naming neither a family nor a value records nothing.
    #[test]
    fn an_instance_naming_neither_a_family_nor_a_value_records_nothing() {
        pretty_assertions::assert_eq!(
            read(&json!({expanded_properties::OBSERVED_AT.as_str(): date_time("2024-01-01T00:00:00Z")})),
            None
        );
    }

    /// A deletion's `observedAt` is held to the clause 4.6.3 strict UTC form
    /// like every other client `DateTime`.
    #[test]
    fn a_deletion_observed_at_outside_the_strict_utc_form_is_refused() {
        let attribute: AttributeName = "https://example.org/speed".parse().expect("a valid attribute name");
        let instance = json!({
            "@type": [PROPERTY],
            expanded_properties::VALUE.as_str(): [{"@value": "urn:ngsi-ld:null"}],
            expanded_properties::OBSERVED_AT.as_str(): [{"@value": "2024-01-01T01:00:00+01:00"}]
        });
        let error = InstanceDocument::from_slice(&attribute, &serde_json::to_vec(&instance).expect("JSON")).expect_err("a numeric offset is not the UTC form");
        assert!(error.to_string().contains("observedAt"), "{error}");
    }
}
