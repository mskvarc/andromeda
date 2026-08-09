//! Idempotent temporal append sink backed by PostgreSQL history tables.

use crate::{
    error::TemporalStoreError,
    params::{ScopeHistoryEntry, TeaBatchEntry},
    repo::TemporalRepository,
    value_type::ValueType,
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_model::temporal::{TemporalHistoryRow, TemporalValue};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use andromeda_postgresql_context_store::attribute_current::queue::AttributeCurrentQueue;
use andromeda_postgresql_query::history_batch_entry::HistoryBatchEntry;
use andromeda_temporal::{
    append_intent::{TemporalAppendEntry, TemporalAppendIntent, TemporalObservationIdentity},
    error::TemporalError,
    port::temporal_history_sink::{TemporalHistorySink, TemporalSinkAppendOutcome},
};
use async_trait::async_trait;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

/// PostgreSQL temporal history used through the idempotent append port.
pub struct PostgresTemporalHistorySink {
    /// Temporal repository selected for the sink workload.
    repository: Arc<TemporalRepository>,
    /// Projection publication for PostgreSQL current-attribute reads.
    attribute_current: AttributeCurrentQueue,
}

impl PostgresTemporalHistorySink {
    /// Construct one PostgreSQL-backed idempotent history sink.
    pub const fn new(repository: Arc<TemporalRepository>, attribute_current: AttributeCurrentQueue) -> PostgresTemporalHistorySink {
        PostgresTemporalHistorySink { repository, attribute_current }
    }

    /// Append all typed intent entries in the receipt transaction.
    pub(crate) async fn append_entries(
        repository: &TemporalRepository,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        intent: &TemporalAppendIntent,
    ) -> Result<Vec<HistoryBatchEntry>, TemporalStoreError> {
        let tea_entries = Self::tea_entries(repository, intent).await?;
        let tea_results = repository.upsert_tea_batch_in_tx(tx, &tea_entries).await?;
        let tea_ids = tea_results
            .into_iter()
            .map(|row| ((row.attribute_name, row.dataset_id), row.id))
            .collect::<HashMap<_, _>>();
        let mut history = Vec::new();
        let mut scopes = Vec::new();
        for (position, entry) in intent.entries.iter().enumerate() {
            let ordinal =
                i32::try_from(position).map_err(|error| TemporalStoreError::Validation(format!("temporal intent ordinal is out of range: {error}")))?;
            match entry {
                TemporalAppendEntry::Observation {
                    row,
                    client_supplied_instance_id,
                    recorded_at,
                } => {
                    let tea_id = tea_ids
                        .get(&(row.attribute_name.clone(), row.dataset_id.clone())) // clone: lookup key follows the catalogue's owned natural key.
                        .copied()
                        .ok_or_else(|| TemporalStoreError::Query("temporal sink catalogue upsert returned no matching row".to_owned()))?;
                    history.push(history_entry(
                        tea_id,
                        row,
                        *recorded_at,
                        *client_supplied_instance_id,
                        TemporalObservationIdentity {
                            intent_id: intent.id,
                            ordinal: u32::try_from(ordinal).map_err(|error| TemporalStoreError::Validation(error.to_string()))?,
                        },
                    )?);
                }
                TemporalAppendEntry::Tombstone {
                    attribute_name,
                    dataset_id,
                    instance_id,
                    recorded_at,
                } => {
                    let tea_id = tea_ids
                        .get(&(attribute_name.clone(), dataset_id.clone())) // clone: lookup key follows the catalogue's owned natural key.
                        .copied()
                        .ok_or_else(|| TemporalStoreError::Query("temporal sink tombstone catalogue upsert returned no matching row".to_owned()))?;
                    history.push(HistoryBatchEntry {
                        time: *recorded_at,
                        tea_id,
                        value_number: None,
                        value_string: Some(expanded_properties::NULL_URI.as_str().to_owned()),
                        value_bool: None,
                        value_geo: None,
                        value_object: None,
                        value_datetime: None,
                        value_uri: None,
                        instance_id: instance_id.clone(), // clone: the persisted row owns its stable instance identity.
                        observed_at: None,
                        metadata: None,
                        recorded_at: *recorded_at,
                        instance_id_client_supplied: false,
                        intent_id: Some(intent.id.as_uuid()),
                        intent_ordinal: Some(ordinal),
                    });
                }
                TemporalAppendEntry::Scope {
                    path,
                    instance_id,
                    recorded_at,
                } => scopes.push(ScopeHistoryEntry {
                    time: *recorded_at,
                    entity_id: intent.entity_id.clone(), // clone: each persisted scope row owns its entity identity.
                    scope_path: path.clone(),            // clone: each persisted scope row owns its validated path.
                    instance_id: instance_id.clone(),    // clone: each persisted scope row owns its instance identity.
                }),
            }
        }
        repository.insert_history_batch_in_tx(tx, &history).await?;
        repository.insert_scope_history_batch_in_tx(tx, &scopes).await?;
        Ok(history)
    }

    /// Build catalogue rows, retaining existing tombstone attribute types.
    async fn tea_entries(repository: &TemporalRepository, intent: &TemporalAppendIntent) -> Result<Vec<TeaBatchEntry>, TemporalStoreError> {
        let mut entries = Vec::new();
        for entry in &intent.entries {
            match entry {
                TemporalAppendEntry::Observation { row, .. } => entries.push(TeaBatchEntry {
                    entity_id: intent.entity_id.clone(),        // clone: the catalogue row owns the entity identity.
                    attribute_name: row.attribute_name.clone(), // clone: the catalogue row owns the attribute identity.
                    attribute_type: row.attribute_type,
                    dataset_id: row.dataset_id.clone(), // clone: the catalogue row owns the dataset identity.
                    value_type: value_type(&row.value),
                }),
                TemporalAppendEntry::Tombstone {
                    attribute_name, dataset_id, ..
                } => {
                    let existing = repository.get_teas_for_entity_attr(&intent.entity_id, attribute_name).await?;
                    let matching = existing
                        .iter()
                        .find(|row| row.dataset_id.as_ref() == dataset_id.as_ref())
                        .or_else(|| existing.first());
                    entries.push(TeaBatchEntry {
                        entity_id: intent.entity_id.clone(),    // clone: the catalogue row owns the entity identity.
                        attribute_name: attribute_name.clone(), // clone: the catalogue row owns the attribute identity.
                        attribute_type: matching.map_or(AttributeType::Property, |row| row.attribute_type),
                        dataset_id: dataset_id.clone(), // clone: the catalogue row owns the dataset identity.
                        value_type: matching.map_or(ValueType::String, |row| row.value_type),
                    });
                }
                TemporalAppendEntry::Scope { .. } => {}
            }
        }
        Ok(entries)
    }
}

#[async_trait]
impl TemporalHistorySink for PostgresTemporalHistorySink {
    async fn append(&self, intent: &TemporalAppendIntent) -> Result<TemporalSinkAppendOutcome, TemporalError> {
        let mut tx = self.repository.pool.begin().await.map_err(persistence)?;
        let receipt = sqlx::query("INSERT INTO temporal_history_sink_receipts (intent_id) VALUES ($1) ON CONFLICT DO NOTHING RETURNING intent_id")
            .bind(intent.id.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(persistence)?;
        if receipt.is_none() {
            return Ok(TemporalSinkAppendOutcome::AlreadyPresent);
        }
        let history = PostgresTemporalHistorySink::append_entries(&self.repository, &mut tx, intent)
            .await
            .map_err(persistence)?;
        let reserved = self.attribute_current.reserve(history);
        match tx.commit().await {
            Ok(()) => {
                if let Some(reserved) = reserved {
                    reserved.enqueue().await;
                }
                Ok(TemporalSinkAppendOutcome::Appended)
            }
            Err(error) => {
                if let Some(reserved) = reserved {
                    reserved.abandon().await;
                }
                Err(persistence(error))
            }
        }
    }
}

fn history_entry(
    tea_id: uuid::Uuid,
    row: &TemporalHistoryRow,
    recorded_at: chrono::DateTime<chrono::Utc>,
    client_supplied_instance_id: bool,
    identity: TemporalObservationIdentity,
) -> Result<HistoryBatchEntry, TemporalStoreError> {
    let mut entry = HistoryBatchEntry {
        time: row.time,
        tea_id,
        value_number: None,
        value_string: None,
        value_bool: None,
        value_geo: None,
        value_object: None,
        value_datetime: None,
        value_uri: None,
        instance_id: row.instance_id.clone(), // clone: the persisted row owns its instance identity.
        observed_at: row.observed_at,
        metadata: row.metadata.clone(), // clone: the persisted row owns its metadata.
        recorded_at,
        instance_id_client_supplied: client_supplied_instance_id,
        intent_id: Some(identity.intent_id.as_uuid()),
        intent_ordinal: Some(i32::try_from(identity.ordinal).map_err(|error| TemporalStoreError::Validation(error.to_string()))?),
    };
    match &row.value {
        TemporalValue::Number(value) => entry.value_number = Some(*value),
        TemporalValue::String(value) => entry.value_string = Some(value.clone()), // clone: the persisted row owns the string.
        TemporalValue::Boolean(value) => entry.value_bool = Some(*value),
        TemporalValue::Geometry(value) => {
            entry.value_geo = Some(Geometry::from_geojson_value(value).map_err(|error| TemporalStoreError::Validation(error.to_string()))?);
        }
        TemporalValue::Object(value) => entry.value_object = Some(Value::clone(value)), // clone: the persisted row owns the structured value.
        TemporalValue::DateTime(value) => entry.value_datetime = Some(*value),
        TemporalValue::Uri(value) => entry.value_uri = Some(value.clone()), // clone: the persisted row owns the URI.
        TemporalValue::Tombstone => entry.value_string = Some(expanded_properties::NULL_URI.as_str().to_owned()),
    }
    Ok(entry)
}

const fn value_type(value: &TemporalValue) -> ValueType {
    match value {
        TemporalValue::Number(_) => ValueType::Number,
        TemporalValue::String(_) | TemporalValue::Tombstone => ValueType::String,
        TemporalValue::Boolean(_) => ValueType::Boolean,
        TemporalValue::Geometry(_) => ValueType::Geometry,
        TemporalValue::Object(_) => ValueType::Object,
        TemporalValue::DateTime(_) => ValueType::DateTime,
        TemporalValue::Uri(_) => ValueType::Uri,
    }
}

fn persistence(error: impl std::fmt::Display) -> TemporalError {
    TemporalError::persistence(error.to_string())
}
