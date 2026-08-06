//! Writing attribute history whenever entities are created or modified.

use crate::{
    core::error::AndromedaError,
    repository::temporal::{
        attribute_instance::{collect_metadata, detect_attribute_type, extract_observed_at, generated_instance_id},
        attribute_value::{HistoryValueColumns, extract_attribute_value},
        entry::TeaEntry,
        observation::{PreparedObservations, build_history_entries, prepare_from_shared},
        params::{HistoryBatchEntry, HistoryValues, TeaBatchEntry, UpdateInstanceParams},
        repo::TemporalRepository,
        rows::TeaBatchResultRow,
        value_type::ValueType,
    },
};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
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
    pub async fn get_teas_for_entity_attr(&self, entity_id: &str, attr_iri: &str) -> Result<Vec<TeaEntry>, AndromedaError> {
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
        entities: &[(Arc<str>, Arc<Value>)],
    ) -> Result<Vec<HistoryBatchEntry>, AndromedaError> {
        self.flush_prepared_in_tx(tx, prepare_from_shared(entities)).await
    }

    async fn flush_prepared_in_tx(&self, tx: &mut Transaction<'_, Postgres>, prepared: PreparedObservations) -> Result<Vec<HistoryBatchEntry>, AndromedaError> {
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
        entity_id: &str,
        attr_iri: &str,
        dataset_id: Option<&str>,
    ) -> Result<Vec<HistoryBatchEntry>, AndromedaError> {
        let (tea_entry, history_entry) = self.build_deletion_entries(entity_id, attr_iri, dataset_id).await?;
        let tea_results = self.repo.upsert_tea_batch_in_tx(tx, from_ref(&tea_entry)).await?;
        let mut history = history_entry;
        history.tea_id = first_tea_id(tea_results)?;
        self.repo.insert_history_batch_in_tx(tx, from_ref(&history)).await?;
        Ok(vec![history])
    }

    async fn build_deletion_entries(
        &self,
        entity_id: &str,
        attr_iri: &str,
        dataset_id: Option<&str>,
    ) -> Result<(TeaBatchEntry, HistoryBatchEntry), AndromedaError> {
        let teas = self.repo.get_teas_for_entity_attr(entity_id, attr_iri).await?;
        let existing = teas
            .iter()
            .find(|t| t.dataset_id.as_ref().map(IriBuf::as_str) == dataset_id)
            .or_else(|| teas.first());
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
            entity_id: entity_id.to_owned(),
            attribute_name: attr_iri.to_owned(),
            attribute_type: attr_type,
            dataset_id: dataset_id.map(ToOwned::to_owned),
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
            instance_id: generated_instance_id(),
            observed_at: None,
            metadata: None,
            recorded_at: now,
            instance_id_client_supplied: false,
        };
        Ok((tea_entry, history_entry))
    }

    /// Update an existing attribute instance (full replacement, preserves `createdAt`).
    pub async fn update_attribute_instance(&self, entity_id: &str, attr_name: &str, instance_id: &str, instance: &Value) -> Result<(), AndromedaError> {
        let (owned, observed_at) = build_update_instance_state(instance)?;
        let params = build_update_instance_params(instance_id, entity_id, attr_name, &owned, observed_at);
        self.repo.update_instance(params).await
    }
}

/// The owned halves of an instance update, kept alive while the borrowed
/// parameter struct that points into them is built and executed.
struct UpdateInstanceOwned {
    columns: HistoryValueColumns,
    metadata: Option<Value>,
}

fn build_update_instance_state(instance: &Value) -> Result<(UpdateInstanceOwned, Option<DateTime<Utc>>), AndromedaError> {
    let attribute_type = detect_attribute_type(instance);
    let attribute_value = extract_attribute_value(instance, attribute_type)?;
    let observed_at = extract_observed_at(instance).and_then(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)).ok());
    let metadata = collect_metadata(instance);
    Ok((
        UpdateInstanceOwned {
            columns: attribute_value.into_columns(),
            metadata,
        },
        observed_at,
    ))
}

fn build_update_instance_params<'a>(
    instance_id: &'a str,
    entity_id: &'a str,
    attribute_name: &'a str,
    owner: &'a UpdateInstanceOwned,
    observed_at: Option<DateTime<Utc>>,
) -> UpdateInstanceParams<'a> {
    UpdateInstanceParams {
        instance_id,
        entity_id,
        attribute_name,
        values: HistoryValues {
            value_number: owner.columns.value_number,
            value_string: owner.columns.value_string.as_deref(),
            value_bool: owner.columns.value_bool,
            value_geo: owner.columns.value_geo.as_ref(),
            value_object: owner.columns.value_object.as_ref(),
            value_datetime: owner.columns.value_datetime,
            value_uri: owner.columns.value_uri.as_deref(),
            metadata: owner.metadata.as_ref(),
        },
        observed_at,
    }
}

fn first_tea_id(tea_results: Vec<TeaBatchResultRow>) -> Result<Uuid, AndromedaError> {
    tea_results
        .into_iter()
        .next()
        .map(|r| r.id)
        .ok_or_else(|| AndromedaError::internal("upsert_tea_batch returned no rows".to_string()))
}
