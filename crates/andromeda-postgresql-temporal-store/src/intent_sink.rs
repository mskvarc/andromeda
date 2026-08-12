//! Idempotent temporal append sink backed by PostgreSQL history tables.

use crate::{
    attribute_history_row::AttributeHistoryRow,
    attribute_history_value::AttributeHistoryValue,
    attribute_kind_code::stored_attribute_kind,
    error::TemporalStoreError,
    repo::TemporalRepository,
    scope_history_entry::ScopeHistoryEntry,
    scope_history_value::ScopeHistoryValue,
    semantic_time_axes::SemanticTimeAxes,
    temporal_stream::{TemporalCatalogUpsert, TemporalStreamKey},
    unit_code_metadata::take_unit_code,
    write_record_persist::persist_write_record,
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::entity_member::extract_types,
    temporal::{TemporalHistoryRow, TemporalValue},
};
use andromeda_postgresql_codes::stream_id::StreamId;
use andromeda_temporal::{
    append_intent::{TemporalAppendEntry, TemporalAppendIntent, TemporalObservationIdentity},
    error::TemporalError,
    port::temporal_history_sink::TemporalHistorySink,
    scope_observation::ScopeObservation,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

/// PostgreSQL temporal history used through the idempotent append port.
///
/// The sink writes Temporal Evolution and nothing else. Current entity state
/// and its `current_attribute_values` projection are written together by the
/// transaction that mutates the entity, so a deferred history append neither
/// produces nor publishes projection work: an append that reaches this sink
/// describes an entity mutation that has already committed.
pub struct PostgresTemporalHistorySink {
    /// Temporal repository selected for the sink workload.
    repository: Arc<TemporalRepository>,
}

impl PostgresTemporalHistorySink {
    /// Construct one PostgreSQL-backed idempotent history sink.
    pub const fn new(repository: Arc<TemporalRepository>) -> PostgresTemporalHistorySink {
        PostgresTemporalHistorySink { repository }
    }

    /// Append all typed intent entries in the caller's transaction.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::NotFound`] when the intent extends a
    /// Temporal Evolution that does not exist and declares no entity type to
    /// create it with, [`TemporalStoreError::Validation`] when an entry cannot
    /// be encoded, and [`TemporalStoreError::Database`] when a statement fails.
    pub(crate) async fn append_entries(
        repository: &TemporalRepository,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        intent: &TemporalAppendIntent,
    ) -> Result<(), TemporalStoreError> {
        let recorded_at = intent.record.recorded_at();
        let streams = repository.upsert_temporal_catalog_in_tx(tx, &catalog_upsert(intent), recorded_at).await?;
        let mut history = Vec::new();
        let mut scopes = Vec::new();
        for (position, entry) in intent.entries.iter().enumerate() {
            let delta = u32::try_from(position)
                .map_err(|error| TemporalStoreError::Validation(format!("temporal intent entry position {position} is out of range: {error}")))?;
            let ordinal = intent
                .base
                .advanced_by(delta)
                .map_err(|error| TemporalStoreError::Validation(format!("temporal intent entry ordinal at position {position} overflowed: {error}")))?;
            let identity = TemporalObservationIdentity {
                write_id: intent.record.write_id(),
                ordinal,
            };
            match entry {
                TemporalAppendEntry::Observation { row } => {
                    let stream_id = resolve_stream(&streams, &row.attribute_name, row.dataset_id.as_ref())?;
                    history.push(observation_row(stream_id, row, recorded_at, identity)?);
                }
                TemporalAppendEntry::Tombstone {
                    attribute_name,
                    dataset_id,
                    attribute_type,
                    instance,
                } => {
                    let stream_id = resolve_stream(&streams, attribute_name, dataset_id.as_ref())?;
                    history.push(AttributeHistoryRow {
                        recorded_at,
                        identity,
                        stream_id,
                        // The family the ended instance belonged to is carried on
                        // the tombstone, which is what lets the render synthesise
                        // the null member that family prescribes.
                        attribute_kind: stored_attribute_kind(*attribute_type),
                        // A deletion records its instant and no typed value; the
                        // null representation is synthesised from the attribute
                        // family at render time (clauses 4.5.7 and 4.5.8).
                        value: AttributeHistoryValue::Tombstone { deleted_at: recorded_at },
                        axes: SemanticTimeAxes::default(),
                        instance: instance.clone(), // clone: the persisted row owns its stable instance identity.
                        unit_code_id: None,
                        metadata: None,
                    });
                }
                TemporalAppendEntry::Scope { observation, instance } => scopes.push(ScopeHistoryEntry {
                    recorded_at,
                    entity_id: intent.entity_id.clone(), // clone: each persisted scope row owns its entity identity.
                    identity,
                    value: match observation {
                        // clone: each persisted scope row owns its validated path.
                        ScopeObservation::Path(path) => ScopeHistoryValue::Path(path.clone()),
                        // Clause 4.18 reserves `urn:ngsi-ld:null` for a deleted
                        // scope, which the row records as a deletion instant with
                        // no path rather than as a sentinel path.
                        ScopeObservation::Deleted => ScopeHistoryValue::Tombstone { deleted_at: recorded_at },
                    },
                    axes: SemanticTimeAxes::default(),
                    instance: instance.clone(), // clone: each persisted scope row owns its instance identity.
                }),
            }
        }
        repository.insert_history_batch_in_tx(tx, &history).await?;
        repository.insert_scope_history_batch_in_tx(tx, &scopes).await?;
        Ok(())
    }
}

#[async_trait]
impl TemporalHistorySink for PostgresTemporalHistorySink {
    async fn append(&self, intent: &TemporalAppendIntent) -> Result<(), TemporalError> {
        let mut tx = self.repository.pool.begin().await.map_err(persistence)?;
        persist_write_record(&mut tx, &intent.record).await.map_err(persistence)?;
        // No claim gate: a replay of the same intent always reaches this call
        // again, and its rows collide with the ones the first attempt wrote on
        // their row-level `ON CONFLICT ... DO NOTHING` constraints instead of
        // duplicating them.
        PostgresTemporalHistorySink::append_entries(&self.repository, &mut tx, intent)
            .await
            .map_err(persistence)?;
        tx.commit().await.map_err(persistence)?;
        Ok(())
    }
}

/// The catalogue rows one intent needs before its history rows can be stored.
///
/// Entity types come from the observation payload the intent was prepared from,
/// which is the only place a write states what the entity is; an intent that
/// carries none — an attribute deletion, say — extends a Temporal Evolution that
/// must already exist.
fn catalog_upsert(intent: &TemporalAppendIntent) -> TemporalCatalogUpsert {
    let types = intent.selection_value.as_ref().map(extract_types).unwrap_or_default();
    let mut streams: Vec<TemporalStreamKey> = Vec::with_capacity(intent.entries.len());
    for entry in &intent.entries {
        let key = match entry {
            TemporalAppendEntry::Observation { row } => TemporalStreamKey {
                attribute_name: row.attribute_name.clone(), // clone: the catalogue grain owns the attribute identity.
                dataset_id: row.dataset_id.clone(),         // clone: the catalogue grain owns the dataset identity.
            },
            TemporalAppendEntry::Tombstone {
                attribute_name, dataset_id, ..
            } => TemporalStreamKey {
                attribute_name: attribute_name.clone(), // clone: the catalogue grain owns the attribute identity.
                dataset_id: dataset_id.clone(),         // clone: the catalogue grain owns the dataset identity.
            },
            TemporalAppendEntry::Scope { .. } => continue,
        };
        streams.push(key);
    }
    TemporalCatalogUpsert {
        entity_id: intent.entity_id.clone(), // clone: the catalogue upsert owns the entity identity.
        types,
        streams,
    }
}

/// Find the stream the catalogue upsert assigned to one attribute grain.
fn resolve_stream(
    streams: &HashMap<TemporalStreamKey, StreamId>,
    attribute_name: &AttributeName,
    dataset_id: Option<&Uri>,
) -> Result<StreamId, TemporalStoreError> {
    let key = TemporalStreamKey {
        attribute_name: attribute_name.clone(), // clone: the lookup key follows the catalogue's owned natural key.
        dataset_id: dataset_id.cloned(),
    };
    streams
        .get(&key)
        .copied()
        .ok_or_else(|| TemporalStoreError::Query("temporal sink catalogue upsert returned no matching stream".to_owned()))
}

/// Build one history row from a neutral observation.
fn observation_row(
    stream_id: StreamId,
    row: &TemporalHistoryRow,
    recorded_at: DateTime<Utc>,
    identity: TemporalObservationIdentity,
) -> Result<AttributeHistoryRow, TemporalStoreError> {
    let mut metadata = row.metadata.clone(); // clone: the persisted row owns its metadata.
    let unit_code_id = take_unit_code(&mut metadata)?;
    Ok(AttributeHistoryRow {
        recorded_at,
        identity,
        stream_id,
        attribute_kind: stored_attribute_kind(row.attribute_type),
        value: stored_value(row)?,
        axes: SemanticTimeAxes {
            // Each axis is preserved exactly as the observation carried it. An
            // observation that declared none keeps none, so it matches no window
            // on that Temporal Property (clauses 4.11 and 5.2.21).
            observed_at: row.observed_at,
            created_at: row.created_at,
            modified_at: row.modified_at,
        },
        instance: row.instance.clone(), // clone: the persisted row owns its instance identity.
        unit_code_id,
        metadata,
    })
}

/// Translate a neutral observation value into the shape the row stores.
fn stored_value(row: &TemporalHistoryRow) -> Result<AttributeHistoryValue, TemporalStoreError> {
    match &row.value {
        TemporalValue::Number(value) => Ok(AttributeHistoryValue::Number(*value)),
        TemporalValue::String(value) => Ok(AttributeHistoryValue::String(value.clone())), // clone: the persisted row owns the string.
        TemporalValue::Boolean(value) => Ok(AttributeHistoryValue::Boolean(*value)),
        TemporalValue::Geometry(value) => Ok(AttributeHistoryValue::Geometry(
            Geometry::from_geojson_value(value).map_err(|error| TemporalStoreError::Validation(error.to_string()))?,
        )),
        TemporalValue::Object(value) => Ok(AttributeHistoryValue::Object(Value::clone(value))), // clone: the persisted row owns the structured value.
        TemporalValue::DateTime(value) => Ok(AttributeHistoryValue::DateTime(*value)),
        TemporalValue::Uri(value) => Ok(AttributeHistoryValue::Uri(value.clone())), // clone: the persisted row owns the URI.
        TemporalValue::Tombstone { deleted_at } => Ok(AttributeHistoryValue::Tombstone { deleted_at: *deleted_at }),
    }
}

/// Classify one store failure at the temporal port boundary.
fn persistence(error: impl std::fmt::Display) -> TemporalError {
    TemporalError::persistence(error.to_string())
}
