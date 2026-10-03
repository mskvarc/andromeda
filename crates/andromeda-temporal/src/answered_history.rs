//! The instances a Context Source answered a temporal query with, read as the
//! history rows the store aggregates (clauses 4.5.19 and 5.7.4.4).
//!
//! An aggregated representation of an entity held in several parts aggregates
//! the instances of every part, so the instances a source answered with are
//! staged beside the local ones and aggregated by the same statement. Each
//! answered instance is read exactly as a written one is: expanded on its own,
//! so the instant it carries on every Temporal Property is the one it carries,
//! and read by [`ObservedInstance::read`], which decides its family, dataset,
//! value and metadata as the write path does. An instance whose value is its
//! family's deletion encoding observes nothing, so it records no row and
//! aggregates nothing.

use crate::{error::TemporalError, observed_instance::ObservedInstance, temporal_validation_reason::TemporalValidationReason};
use andromeda_ngsi_ld_entity::{document::instance_document::InstanceDocument, system_instant::SystemInstant};
use andromeda_ngsi_ld_expansion::{entity_expansion::EntityExpansion, normalization::Normalization};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    entity_type_name::EntityTypeName,
    instance_family::InstanceFamily,
    instance_identity::InstanceIdentity,
};
use andromeda_ngsi_ld_model::temporal::TemporalHistoryRow;
use andromeda_ngsi_ld_representation::entity_members::ENTITY_MEMBERS;
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use std::slice::from_ref;

/// The history one answered entity holds, and the types it names.
#[derive(Debug, Default)]
pub(crate) struct AnsweredHistory {
    /// One row per instance the answer observes.
    pub(crate) rows: Vec<TemporalHistoryRow>,
    /// The entity's types, expanded.
    pub(crate) entity_types: Vec<EntityTypeName>,
}

/// Read the normalized temporal entity `answer` under `expansion` as the rows
/// its instances record, each stamped `recorded_at`.
///
/// # Errors
///
/// Returns the errors of expanding and reading a written instance when an
/// instance of the answer is none the model admits.
pub(crate) async fn answered_history(expansion: &EntityExpansion<'_>, answer: &Value, recorded_at: DateTime<Utc>) -> Result<AnsweredHistory, TemporalError> {
    let Some(members) = answer.as_object() else {
        return Ok(AnsweredHistory::default());
    };
    let entity_types = answered_types(expansion, members).await?;
    let mut rows = Vec::new();
    for (key, member) in members.iter().filter(|(key, _)| !ENTITY_MEMBERS.contains(&key.as_str())) {
        let written_name: AttributeName = key
            .parse()
            .map_err(|source| TemporalError::from(TemporalValidationReason::AttributeName { source }))?;
        let instances = match member {
            Value::Array(instances) => instances.as_slice(),
            object @ Value::Object(_) => from_ref(object),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => &[],
        };
        for instance in instances {
            // clone: the expansion consumes the instance, which the answer keeps
            let (attribute_name, document) = expansion.attribute_fragment(&written_name, instance.clone()).await?.into_parts();
            rows.extend(instance_row(attribute_name, document, recorded_at)?);
        }
    }
    Ok(AnsweredHistory { rows, entity_types })
}

/// The expanded types the answer names.
async fn answered_types(expansion: &EntityExpansion<'_>, members: &Map<String, Value>) -> Result<Vec<EntityTypeName>, TemporalError> {
    let identity: Map<String, Value> = members
        .iter()
        .filter(|(key, _)| matches!(key.as_str(), "id" | "type"))
        .map(|(key, value)| (key.clone(), value.clone())) // clone: the expansion consumes the members the answer keeps
        .collect();
    let document = expansion.entity_fragment(Value::Object(identity), Normalization::Entity).await?;
    Ok(document.types().to_vec())
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
        instance: instance_id.map_or_else(InstanceIdentity::generate, |provided| {
            InstanceIdentity::from_identifier(provided, InstanceFamily::Attribute)
        }),
        metadata,
        partition_total: None,
    }))
}

#[cfg(test)]
mod tests {
    use crate::answered_history::answered_history;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_expansion::entity_expansion::EntityExpansion;
    use andromeda_ngsi_ld_model::temporal::TemporalHistoryRow;
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};

    fn at(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("a valid instant").with_timezone(&Utc)
    }

    async fn rows_of(answer: &Value) -> Vec<TemporalHistoryRow> {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("build engine");
        let context = ContextSpec::from_body(json!({"speed": "https://example.com/speed"}));
        let expansion = EntityExpansion::prepare(&engine, &context).await.expect("the context resolves");
        answered_history(&expansion, answer, at("2030-01-01T00:00:00Z"))
            .await
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
}
