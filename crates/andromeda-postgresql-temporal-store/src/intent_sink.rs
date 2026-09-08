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
    temporal_catalog_write::TemporalCatalog,
    temporal_stream::{TemporalCatalogUpsert, TemporalStreamKey},
    unit_code_metadata::take_unit_code,
    write_record_persist::persist_write_record,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::entity_member::extract_types,
    temporal::{TemporalHistoryRow, TemporalValue},
};
use andromeda_postgresql_codes::{stream_key::StreamKey, temporal_entity_key::TemporalEntityKey};
use andromeda_temporal::{
    append_intent::{TemporalAppendEntry, TemporalAppendIntent, TemporalObservationIdentity},
    error::TemporalError,
    port::temporal_history_sink::TemporalHistorySink,
    scope_observation::ScopeObservation,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{fmt::Display, slice, sync::Arc};

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
    #[must_use]
    pub const fn new(repository: Arc<TemporalRepository>) -> PostgresTemporalHistorySink {
        PostgresTemporalHistorySink { repository }
    }

    /// Append a whole chunk of typed intents in the caller's transaction.
    ///
    /// The catalogue is resolved once for the chunk — every entity's narrow key
    /// and every grain's stream in a fixed number of statements regardless of
    /// how many entities the chunk carries — then the history and scope rows of
    /// every intent are built in memory against that one catalogue and landed in
    /// one history insert and one scope insert. Per-intent ordinal derivation
    /// stays per-intent; only the statements are hoisted to the chunk.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::NotFound`] when an intent extends a
    /// Temporal Evolution that does not exist and declares no entity type to
    /// create it with, [`TemporalStoreError::Validation`] when an entry cannot
    /// be encoded, and [`TemporalStoreError::Database`] when a statement fails.
    pub(crate) async fn append_entries_batch(
        repository: &TemporalRepository,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        target: &ContextTarget,
        intents: &[TemporalAppendIntent],
    ) -> Result<(), TemporalStoreError> {
        if intents.is_empty() {
            return Ok(());
        }
        let upserts: Vec<TemporalCatalogUpsert> = intents.iter().map(catalog_upsert).collect();
        // Every intent in the chunk shares its commit's one ingestion instant:
        // the native path hands one record to the whole chunk, and the single
        // deferred path a one-element chunk, so the catalogue's write clock is
        // that instant.
        let recorded_at = intents[0].record.recorded_at();
        let catalog = repository.upsert_temporal_catalog_batch_in_tx(tx, target, &upserts, recorded_at).await?;
        let mut history = Vec::new();
        let mut scopes = Vec::new();
        for intent in intents {
            let recorded_at = intent.record.recorded_at();
            let entity_key = catalog.entity_keys.get(&intent.entity_id).copied();
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
                        let stream_key = resolve_stream(&catalog, entity_key, &intent.entity_id, &row.attribute_name, row.dataset_id.as_ref())?;
                        history.push(observation_row(stream_key, row, recorded_at, identity)?);
                    }
                    TemporalAppendEntry::Tombstone {
                        attribute_name,
                        dataset_id,
                        attribute_type,
                        instance,
                    } => {
                        let stream_key = resolve_stream(&catalog, entity_key, &intent.entity_id, attribute_name, dataset_id.as_ref())?;
                        history.push(AttributeHistoryRow {
                            recorded_at,
                            identity,
                            stream_key,
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
                    TemporalAppendEntry::Scope {
                        observation,
                        observed_at,
                        instance,
                    } => scopes.push(ScopeHistoryEntry {
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
                        // Clause 4.5.6: the temporal scope carries `observedAt`,
                        // `createdAt` and `modifiedAt`; a scope appended by this
                        // write is created and last modified at the write.
                        axes: SemanticTimeAxes {
                            observed_at: *observed_at,
                            created_at: Some(recorded_at),
                            modified_at: Some(recorded_at),
                        },
                        instance: instance.clone(), // clone: each persisted scope row owns its instance identity.
                    }),
                }
            }
        }
        repository.insert_history_batch_in_tx(tx, target, &history).await?;
        repository.insert_scope_history_batch_in_tx(tx, target, &scopes).await?;
        Ok(())
    }
}

#[async_trait]
impl TemporalHistorySink for PostgresTemporalHistorySink {
    async fn append(&self, target: &ContextTarget, intent: &TemporalAppendIntent) -> Result<(), TemporalError> {
        let mut tx = self.repository.pool.begin().await.map_err(persistence)?;
        persist_write_record(&mut tx, &intent.record).await.map_err(persistence)?;
        // No claim gate: a replay of the same intent always reaches this call
        // again, and its rows collide with the ones the first attempt wrote on
        // their row-level `ON CONFLICT ... DO NOTHING` constraints instead of
        // duplicating them. The one intent is a one-element chunk to the batch
        // path the native commit drives many entities through.
        PostgresTemporalHistorySink::append_entries_batch(&self.repository, &mut tx, target, slice::from_ref(intent))
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

/// Find the stream the chunk's catalogue assigned to one entity's attribute
/// grain.
///
/// A grain resolves against the entity it belongs to: the chunk catalogue keys
/// each stream by the narrow entity key beside the grain, so a history row needs
/// its entity's key before it can name its stream. An entity with no resolved
/// key never reaches here for an observation or tombstone — the catalogue
/// resolves every entity a chunk carries — so a missing key is a catalogue
/// invariant violation rather than a routine outcome.
fn resolve_stream(
    catalog: &TemporalCatalog,
    entity_key: Option<TemporalEntityKey>,
    entity_id: &Uri,
    attribute_name: &AttributeName,
    dataset_id: Option<&Uri>,
) -> Result<StreamKey, TemporalStoreError> {
    let entity_key = entity_key.ok_or_else(|| TemporalStoreError::Query(format!("temporal catalogue resolved no narrow key for entity {entity_id}")))?;
    let key = TemporalStreamKey {
        attribute_name: attribute_name.clone(), // clone: the lookup key follows the catalogue's owned natural key.
        dataset_id: dataset_id.cloned(),
    };
    catalog
        .streams
        .get(&(entity_key, key))
        .copied()
        .ok_or_else(|| TemporalStoreError::Query("temporal sink catalogue upsert returned no matching stream".to_owned()))
}

/// Build one history row from a neutral observation.
fn observation_row(
    stream_key: StreamKey,
    row: &TemporalHistoryRow,
    recorded_at: DateTime<Utc>,
    identity: TemporalObservationIdentity,
) -> Result<AttributeHistoryRow, TemporalStoreError> {
    let mut metadata = row.metadata.clone(); // clone: the persisted row owns its metadata.
    let unit_code_id = take_unit_code(&mut metadata)?;
    Ok(AttributeHistoryRow {
        recorded_at,
        identity,
        stream_key,
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
        TemporalValue::Number(value) => Ok(AttributeHistoryValue::Number(value.clone())), // clone: the persisted row owns its exact decimal.
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
fn persistence(error: impl Display) -> TemporalError {
    TemporalError::persistence(error.to_string())
}
