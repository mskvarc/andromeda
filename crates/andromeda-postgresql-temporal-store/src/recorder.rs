//! Writing attribute history whenever entities are created or modified.

use crate::{
    attribute_instance::generated_instance_id,
    entry::TeaEntry,
    error::TemporalStoreError,
    observation::{PreparedObservations, build_history_entries, prepare_from_shared},
    params::TeaBatchEntry,
    repo::TemporalRepository,
    rows::TeaBatchResultRow,
    value_type::ValueType,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, uri::Uri};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use andromeda_postgresql_query::history_batch_entry::HistoryBatchEntry;
use chrono::Utc;
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use std::{slice::from_ref, sync::Arc};
use uuid::Uuid;

/// Records attribute history into the temporal tables.
///
/// Every write method takes the caller's transaction and returns the history
/// rows it wrote, leaving both the commit and the `attribute_current` publish to
/// whatever owns the transaction. The recorder therefore holds no queue handle
/// of its own: a method that committed internally would have to publish its own
/// rows, and could not be composed with the entity write it belongs to.
#[derive(Debug)]
pub struct TemporalRecorder {
    repo: Arc<TemporalRepository>,
}

impl TemporalRecorder {
    /// Create a new temporal recorder backed by the given repository.
    pub const fn new(repo: Arc<TemporalRepository>) -> TemporalRecorder {
        TemporalRecorder { repo }
    }

    /// Look up catalogue entries for a given entity and attribute IRI.
    pub async fn get_teas_for_entity_attr(&self, entity_id: &Uri, attr_iri: &AttributeName) -> Result<Vec<TeaEntry>, TemporalStoreError> {
        self.repo.get_teas_for_entity_attr(entity_id, attr_iri).await
    }

    /// Record all attributes from multiple expanded entities in bulk, inside the
    /// caller-provided transaction.
    ///
    /// Collects all catalogue entries and history rows across all entities, then
    /// executes two bulk statements instead of N * M * 5 sequential round-trips.
    /// Sharing the caller's transaction means the entity write and the temporal
    /// history share a single commit fsync and roll back atomically on any
    /// sub-statement failure.
    ///
    /// Returns the history rows actually written so the caller can queue them
    /// for post-commit `attribute_current` materialisation.
    pub async fn record_entities_batch_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        entities: &[(Arc<Uri>, Arc<Value>)],
    ) -> Result<Vec<HistoryBatchEntry>, TemporalStoreError> {
        self.flush_prepared_in_tx(tx, prepare_from_shared(entities)?).await
    }

    async fn flush_prepared_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        prepared: PreparedObservations,
    ) -> Result<Vec<HistoryBatchEntry>, TemporalStoreError> {
        if prepared.tea_entries.is_empty() && prepared.scope_entries.is_empty() {
            return Ok(Vec::new());
        }

        let tea_results = self.repo.upsert_tea_batch_in_tx(tx, &prepared.tea_entries).await?;
        let history_entries = build_history_entries(&prepared, &tea_results);
        self.repo.insert_history_batch_in_tx(tx, &history_entries).await?;

        if !prepared.scope_entries.is_empty() {
            self.repo.insert_scope_history_batch_in_tx(tx, &prepared.scope_entries).await?;
        }

        Ok(history_entries)
    }

    /// Record a tombstone for a deleted attribute (soft-delete) inside the
    /// caller's transaction.
    ///
    /// Returns the single history row written so the caller can queue it for
    /// post-commit `attribute_current` materialisation.
    pub async fn record_attribute_deletion_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        entity_id: &Uri,
        attr_iri: &AttributeName,
        dataset_id: Option<&Uri>,
    ) -> Result<Vec<HistoryBatchEntry>, TemporalStoreError> {
        let (tea_entry, history_entry) = self.build_deletion_entries(entity_id, attr_iri, dataset_id).await?;
        let tea_results = self.repo.upsert_tea_batch_in_tx(tx, from_ref(&tea_entry)).await?;
        let mut history = history_entry;
        history.tea_id = first_tea_id(tea_results)?;
        self.repo.insert_history_batch_in_tx(tx, from_ref(&history)).await?;
        Ok(vec![history])
    }

    async fn build_deletion_entries(
        &self,
        entity_id: &Uri,
        attr_iri: &AttributeName,
        dataset_id: Option<&Uri>,
    ) -> Result<(TeaBatchEntry, HistoryBatchEntry), TemporalStoreError> {
        let teas = self.repo.get_teas_for_entity_attr(entity_id, attr_iri).await?;
        let existing = teas.iter().find(|entry| entry.dataset_id.as_ref() == dataset_id).or_else(|| teas.first());
        let attr_type = existing.map_or(AttributeType::Property, |t| t.attribute_type);
        // Keep the catalogue describing what the attribute *is*, not what the
        // tombstone row happens to store. The tombstone lives in `value_string`,
        // but writing `ValueType::String` here overwrote the TEA's declared type
        // via the upsert, and the temporal renderer dispatches on that type -- so
        // every pre-deletion numeric observation then rendered with no value at
        // all. Tombstone rows are recognised by their sentinel and short-circuit
        // the type dispatch, so they do not need the type to describe them.
        let value_type = existing.map_or(ValueType::String, |t| t.value_type);

        let tea_entry = TeaBatchEntry {
            entity_id: entity_id.clone(),     // clone: the catalogue row owns its entity URI.
            attribute_name: attr_iri.clone(), // clone: the catalogue row owns its attribute name.
            attribute_type: attr_type,
            dataset_id: dataset_id.cloned(), // clone: the catalogue row owns its dataset URI.
            value_type,
        };
        let now = Utc::now();
        let history_entry = HistoryBatchEntry {
            time: now,
            tea_id: Uuid::nil(),
            value_number: None,
            value_string: Some(expanded_properties::NULL_URI.as_str().to_owned()),
            value_bool: None,
            value_geo: None,
            value_object: None,
            value_datetime: None,
            value_uri: None,
            instance_id: generated_instance_id()?,
            observed_at: None,
            metadata: None,
            recorded_at: now,
            instance_id_client_supplied: false,
        };
        Ok((tea_entry, history_entry))
    }
}

fn first_tea_id(tea_results: Vec<TeaBatchResultRow>) -> Result<Uuid, TemporalStoreError> {
    tea_results
        .into_iter()
        .next()
        .map(|r| r.id)
        .ok_or_else(|| TemporalStoreError::Query("upsert_tea_batch returned no rows".to_string()))
}
