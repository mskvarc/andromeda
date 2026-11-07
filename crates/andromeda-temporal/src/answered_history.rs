//! The instances a Context Source answered a temporal query with, read as the
//! history rows the store aggregates (clauses 4.5.19 and 5.7.4.4).
//!
//! An aggregated representation of an entity held in several parts aggregates
//! the instances of every part, so the instances a source answered with are
//! staged beside the local ones and aggregated by the same statement. Each
//! answered instance is read exactly as a written one is: it was expanded on
//! its own, so the instant it carries on every Temporal Property is the one it
//! carries, and read by [`ObservedInstance::read`], which decides its family,
//! dataset, value and metadata as the write path does. An instance whose value
//! is its family's deletion encoding observes nothing, so it records no row
//! and aggregates nothing.

use crate::{error::TemporalError, observed_instance::ObservedInstance};
use andromeda_ngsi_ld_entity::document::{EntityDocumentParts, attribute_member::AttributeMember, instance_document::InstanceDocument};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    entity_type_name::EntityTypeName,
    instance_identity::InstanceIdentity,
    time::system_instant::SystemInstant,
};
use andromeda_ngsi_ld_model::temporal::{StreamExtent, TemporalHistoryRow, evolution_instance::EvolutionInstance, temporal_document::TemporalDocument};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use chrono::{DateTime, Utc};

/// The history one answered entity holds, and the types it names.
#[derive(Debug, Default)]
pub(crate) struct AnsweredHistory {
    /// One row per instance the answer observes.
    pub(crate) rows: Vec<TemporalHistoryRow>,
    /// The entity's types, expanded.
    pub(crate) entity_types: Vec<EntityTypeName>,
}

/// Read the temporal entity `answer` as the rows its instances record, each
/// stamped `recorded_at`.
///
/// The Scope history is the entity's own and observes no attribute, so it
/// records no row.
///
/// # Errors
///
/// Returns the errors of reading a written instance when an instance of the
/// answer is none the model admits.
pub(crate) fn answered_history(answer: TemporalDocument, recorded_at: DateTime<Utc>) -> Result<AnsweredHistory, TemporalError> {
    let (entity, _times, _emptied) = answer.into_parts();
    let EntityDocumentParts { types, attributes, .. } = entity.into_parts();
    let mut rows = Vec::new();
    for (attribute_name, member) in attributes {
        let AttributeMember::Instances(instances) = member else {
            continue;
        };
        if attribute_name.as_str() == expanded_properties::SCOPE.as_str() {
            continue;
        }
        for instance in instances {
            // clone: every row owns the name of the attribute it observes
            rows.extend(instance_row(attribute_name.clone(), instance, recorded_at)?);
        }
    }
    Ok(AnsweredHistory { rows, entity_types: types })
}

/// The row one answered instance records, or none when it observes nothing.
fn instance_row(attribute_name: AttributeName, document: InstanceDocument, recorded_at: DateTime<Utc>) -> Result<Option<TemporalHistoryRow>, TemporalError> {
    let created_at = document.created_at().map(SystemInstant::get);
    let modified_at = document.modified_at().map(SystemInstant::get);
    let Some(observed) = ObservedInstance::read(document)? else {
        return Ok(None);
    };
    let ObservedInstance {
        attribute_type,
        dataset_id,
        value,
        observed_at,
        instance_id,
        metadata,
    } = observed;
    Ok(Some(TemporalHistoryRow {
        attribute_name,
        attribute_type,
        dataset_id,
        value,
        observed_at,
        recorded_at,
        created_at,
        modified_at,
        instance: EvolutionInstance::Attribute(instance_id.map_or_else(InstanceIdentity::generate, InstanceIdentity::from_identifier)),
        metadata,
        stream_extent: StreamExtent::Whole,
    }))
}

#[cfg(test)]
mod tests {
    use crate::{answered_fixture::answered, answered_history::answered_history};
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use andromeda_ngsi_ld_model::temporal::TemporalHistoryRow;
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};

    fn at(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("a valid instant").get()
    }

    async fn rows_of(answer: &Value) -> Vec<TemporalHistoryRow> {
        answered_history(answered(answer).await, at("2030-01-01T00:00:00Z"))
            .expect("the answer reads")
            .rows
    }

    /// Clause 4.8: every instance carries its own system times, and a window
    /// positioned on one reads each instance's own.
    #[tokio::test]
    async fn each_instance_keeps_its_own_axis() {
        let answer = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 10, "modifiedAt": "2020-08-01T10:00:00Z", "instanceId": "urn:ngsi-ld:instance:1"},
                {"type": "Property", "value": 20, "modifiedAt": "2020-08-01T11:00:00Z", "instanceId": "urn:ngsi-ld:instance:2"}
            ]
        });
        let rows = rows_of(&answer).await;
        let axes: Vec<Option<DateTime<Utc>>> = rows.iter().map(|row| row.modified_at).collect();
        pretty_assertions::assert_eq!(axes, vec![Some(at("2020-08-01T10:00:00Z")), Some(at("2020-08-01T11:00:00Z"))]);
        pretty_assertions::assert_eq!(rows[0].attribute_name.as_str(), "https://example.com/speed");
    }

    /// Clause 4.5.0: an instance holding its family's deletion encoding ends an
    /// instance rather than observing one, so it aggregates nothing.
    #[tokio::test]
    async fn a_deletion_records_no_row() {
        let answer = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 10, "observedAt": "2020-08-01T10:00:00Z"},
                {"type": "Property", "value": "urn:ngsi-ld:null", "observedAt": "2020-08-01T11:00:00Z"}
            ]
        });
        pretty_assertions::assert_eq!(rows_of(&answer).await.len(), 1);
    }

    /// The Scope history dates the entity, not an attribute, so it records no
    /// row.
    #[tokio::test]
    async fn the_scope_history_records_no_row() {
        let answer = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "scope": [{"type": "Property", "value": "/Madrid", "observedAt": "2020-08-01T10:00:00Z"}],
            "speed": [{"type": "Property", "value": 10, "observedAt": "2020-08-01T10:00:00Z"}]
        });
        pretty_assertions::assert_eq!(rows_of(&answer).await.len(), 1);
    }
}
