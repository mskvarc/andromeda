//! Shared temporal write-path logic for Entity API and Temporal API.
//!
//! Records attribute history whenever entities are created or modified.

use crate::{
    core::error::AndromedaError,
    domain::{attribute_type::AttributeType, expanded::expanded_geo_node_to_wkb, geometry::Wkb, temporal::ValueType},
    jsonld::vocab,
    query::sql::scope_to_ltree,
    repository::temporal::{
        params::{HistoryBatchEntry, HistoryValues, ScopeHistoryEntry, TeaBatchEntry, UpdateInstanceParams},
        repo::TemporalRepository,
        rows::TeaBatchResultRow,
    },
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;
use urn_rs::Urn;

/// Returns whether the given expanded IRI is an NGSI-LD system attribute.
fn is_system_attribute(attr_name: &str) -> bool {
    matches!(
        attr_name,
        value if value == vocab::expanded::properties::CREATED_AT.as_str()
            || value == vocab::expanded::properties::DATASET_ID.as_str()
            || value == vocab::expanded::properties::DELETED_AT.as_str()
            || value == vocab::expanded::properties::INSTANCE_ID.as_str()
            || value == vocab::expanded::properties::MODIFIED_AT.as_str()
            || value == vocab::expanded::properties::OBSERVED_AT.as_str()
            || value == vocab::expanded::properties::UNIT_CODE.as_str()
    )
}

/// Named parameters for temporal value storage, replacing the 7-element tuple.
pub(crate) struct DbValueParams {
    /// Numeric value for number-typed properties.
    pub value_number: Option<f64>,
    /// String value for string-typed properties.
    pub value_string: Option<String>,
    /// Boolean value for boolean-typed properties.
    pub value_bool: Option<bool>,
    /// Canonical WKB for geo-typed properties.
    pub value_geo: Option<Wkb>,
    /// Arbitrary JSON value for object-typed or complex properties.
    pub value_object: Option<Value>,
    /// Datetime value for datetime-typed properties.
    pub value_datetime: Option<DateTime<Utc>>,
    /// URI string for relationship objects.
    pub value_uri: Option<String>,
}

/// Records attribute history into temporal tables when entities are created or modified.
#[derive(Debug)]
pub struct TemporalRecorder {
    repo: Arc<TemporalRepository>,
}

/// Intermediate batch state: rows collected from the expanded entities but
/// not yet paired with their TEA UUIDs.
struct PreparedBatch {
    tea_entries: Vec<TeaBatchEntry>,
    pending_histories: Vec<PendingHistory>,
    tea_index: Vec<usize>,
    scope_entries: Vec<ScopeHistoryEntry>,
}

/// Per-observation history data that only needs a TEA UUID to become a
/// [`HistoryBatchEntry`].
struct PendingHistory {
    time: DateTime<Utc>,
    db_params: DbValueParams,
    instance_id: String,
    instance_id_client_supplied: bool,
    observed_at: Option<DateTime<Utc>>,
    metadata: Option<Value>,
}

impl TemporalRecorder {
    /// Create a new temporal recorder backed by the given repository.
    pub const fn new(repo: Arc<TemporalRepository>) -> TemporalRecorder {
        TemporalRecorder { repo }
    }

    /// Look up TEA entries for a given entity + attribute IRI.
    pub async fn get_teas_for_entity_attr(&self, entity_id: &str, attr_iri: &str) -> Result<Vec<crate::domain::temporal::TeaEntry>, AndromedaError> {
        self.repo.get_teas_for_entity_attr(entity_id, attr_iri).await
    }

    /// Record all attributes from an expanded JSON-LD entity into temporal tables.
    ///
    /// Invalid attributes are skipped with a warning log rather than failing
    /// the entire operation. This allows valid attributes to be recorded even
    /// when some attributes have issues.
    pub async fn record_entity_attributes(&self, entity_id: &str, expanded: &Value) -> Result<(), AndromedaError> {
        let obj = match expanded {
            Value::Array(arr) => arr.first().and_then(|v| v.as_object()),
            Value::Object(obj) => Some(obj),
            _other => None,
        };

        let Some(obj) = obj else {
            return Ok(());
        };

        for (attr_name, attr_value) in obj {
            if attr_name.starts_with('@') {
                continue;
            }

            // Skip NGSI-LD system-level attributes (observedAt, createdAt, etc.)
            if is_system_attribute(attr_name) {
                continue;
            }

            // Handle scope specially: scopes are NOT attributes -- they go to scope_history,
            // not through the TEA/attribute_history pipeline (NGSI-LD clause 4.5.4).
            if attr_name == vocab::expanded::properties::SCOPE.as_str() {
                if let Some(scope_arr) = attr_value.as_array() {
                    for scope_entry in scope_arr {
                        if let Some(scope_val) = scope_entry.as_object().and_then(|o| o.get("@value")).and_then(|v| v.as_str()) {
                            let entry = ScopeHistoryEntry {
                                time: Utc::now(),
                                entity_id: entity_id.to_string(),
                                scope_path: scope_to_ltree(scope_val),
                                instance_id: format!("urn:ngsi-ld:scope:instance:{}", uuid::Uuid::new_v4()),
                            };
                            if let Err(e) = self.repo.insert_scope_history(&entry).await {
                                tracing::warn!("Failed to record scope history for entity {}: {}", entity_id, e);
                            }
                        }
                    }
                }
                continue;
            }

            let instances = match attr_value {
                Value::Array(arr) => arr,
                _other => continue,
            };

            for instance in instances {
                if let Some(inst_obj) = instance.as_object() {
                    // Skip instances that don't look like NGSI-LD attribute instances
                    // (they must have an @type indicating Property, Relationship, etc.)
                    if !is_ngsi_ld_attribute_instance(inst_obj) {
                        continue;
                    }

                    match self.record_attribute_instance(entity_id, attr_name, &Value::Object(inst_obj.clone())).await {
                        // clone: method requires owned Value; inst_obj is borrowed from instances iterator
                        Ok(()) => {}
                        Err(e) => {
                            tracing::warn!("Failed to record temporal data for entity {}: {}", entity_id, e);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Record a single expanded attribute instance through the batch write path.
    ///
    /// Routes a single observation through [`TemporalRepository::upsert_tea_batch`]
    /// and [`TemporalRepository::insert_history_batch`] so that the single- and
    /// bulk-ingest code paths share identical SQL and dedup semantics.
    pub async fn record_attribute_instance(&self, entity_id: &str, attr_name: &str, instance: &Value) -> Result<(), AndromedaError> {
        let attribute_type = detect_attribute_type(instance);
        let typed_value = extract_typed_value(instance, attribute_type)?;
        let value_type = typed_value.value_type();
        let dataset_id = extract_dataset_id(instance);
        let observed_at_str = extract_observed_at(instance);
        let (instance_id, instance_id_client_supplied) = extract_or_generate_instance_id(instance);
        let metadata = collect_metadata(instance);

        let time = observed_at_str
            .as_ref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).map(|dt| dt.with_timezone(&Utc)).ok())
            .unwrap_or_else(Utc::now);

        let observed_at = observed_at_str
            .as_ref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).map(|dt| dt.with_timezone(&Utc)).ok());

        let dataset_id_str = dataset_id.as_ref().map(ToString::to_string);
        let tea_entries = [TeaBatchEntry {
            entity_id: entity_id.to_owned(),
            attribute_name: attr_name.to_owned(),
            attribute_type,
            dataset_id: dataset_id_str,
            value_type,
        }];
        let tea_results = self.repo.upsert_tea_batch(&tea_entries).await?;
        let tea_id = tea_results
            .into_iter()
            .next()
            .ok_or_else(|| AndromedaError::internal("upsert_tea_batch returned no rows for single entry".to_string()))?
            .id;

        let p = typed_value.into_db_params();

        self.repo
            .insert_history_batch(&[HistoryBatchEntry {
                time,
                tea_id,
                value_number: p.value_number,
                value_string: p.value_string,
                value_bool: p.value_bool,
                value_geo: p.value_geo,
                value_object: p.value_object,
                value_datetime: p.value_datetime,
                value_uri: p.value_uri,
                instance_id,
                observed_at,
                metadata,
                instance_id_client_supplied,
            }])
            .await?;

        Ok(())
    }

    /// Record all attributes from multiple expanded entities in bulk.
    ///
    /// Collects all TEA entries and history rows across all entities, then
    /// executes two bulk SQL operations (TEA upsert + history insert) instead
    /// of N * M * 5 sequential round-trips.
    ///
    /// Invalid attributes are skipped with a warning rather than failing the
    /// entire batch.
    pub async fn record_entities_batch(&self, entities: &[(Arc<str>, Arc<Value>)]) -> Result<(), AndromedaError> {
        let prepared = Self::prepare_batch_arc(entities);
        if prepared.tea_entries.is_empty() && prepared.scope_entries.is_empty() {
            return Ok(());
        }

        let tea_results = self.repo.upsert_tea_batch(&prepared.tea_entries).await?;
        let history_entries = Self::build_history_entries(&prepared, &tea_results);
        self.repo.insert_history_batch(&history_entries).await?;

        if !prepared.scope_entries.is_empty() {
            self.repo.insert_scope_history_batch(&prepared.scope_entries).await?;
        }

        Ok(())
    }

    /// Transactional variant of [`Self::record_entities_batch`]. Writes all
    /// temporal rows inside the caller-provided transaction so the entity
    /// INSERT and temporal history share a single commit fsync and roll
    /// back atomically on any sub-statement failure.
    ///
    /// Returns the history rows actually written so the caller can
    /// queue them with the unit-of-work for post-commit
    /// `attribute_current` materialisation.
    pub async fn record_entities_batch_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        entities: &[(Arc<str>, Arc<Value>)],
    ) -> Result<Vec<HistoryBatchEntry>, AndromedaError> {
        let prepared = Self::prepare_batch_arc(entities);
        self.flush_prepared_in_tx(tx, prepared).await
    }

    /// Transactional variant of [`Self::record_entity_attributes`]. Borrows the
    /// payload directly so the single-entity hot path avoids the `Arc<Value>`
    /// clone the bulk path requires for the durable-queue handoff.
    pub async fn record_entity_attributes_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        entity_id: &str,
        expanded: &Value,
    ) -> Result<Vec<HistoryBatchEntry>, AndromedaError> {
        let prepared = Self::prepare_batch_refs(&[(entity_id, expanded)]);
        self.flush_prepared_in_tx(tx, prepared).await
    }

    async fn flush_prepared_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        prepared: PreparedBatch,
    ) -> Result<Vec<HistoryBatchEntry>, AndromedaError> {
        if prepared.tea_entries.is_empty() && prepared.scope_entries.is_empty() {
            return Ok(Vec::new());
        }

        let tea_results = self.repo.upsert_tea_batch_in_tx(tx, &prepared.tea_entries).await?;
        let history_entries = Self::build_history_entries(&prepared, &tea_results);
        self.repo.insert_history_batch_in_tx(tx, &history_entries).await?;

        if !prepared.scope_entries.is_empty() {
            self.repo.insert_scope_history_batch_in_tx(tx, &prepared.scope_entries).await?;
        }

        Ok(history_entries)
    }

    fn prepare_batch_arc(entities: &[(Arc<str>, Arc<Value>)]) -> PreparedBatch {
        let refs: Vec<(&str, &Value)> = entities.iter().map(|(id, v)| (id.as_ref(), v.as_ref())).collect();
        Self::prepare_batch_refs(&refs)
    }

    fn prepare_batch_refs(entities: &[(&str, &Value)]) -> PreparedBatch {
        let mut tea_entries = Vec::new();
        let mut pending_histories: Vec<PendingHistory> = Vec::new();
        let mut tea_index: Vec<usize> = Vec::new();
        let mut scope_entries: Vec<ScopeHistoryEntry> = Vec::new();

        for (entity_id, expanded) in entities {
            let entity_id_str: &str = entity_id;
            let obj = match *expanded {
                Value::Array(arr) => arr.first().and_then(|v| v.as_object()),
                Value::Object(obj) => Some(obj),
                _other => None,
            };
            let Some(obj) = obj else {
                continue;
            };

            for (attr_name, attr_value) in obj {
                if attr_name.starts_with('@') || is_system_attribute(attr_name) {
                    continue;
                }

                if attr_name == vocab::expanded::properties::SCOPE.as_str() {
                    if let Some(scope_arr) = attr_value.as_array() {
                        for scope_entry in scope_arr {
                            if let Some(scope_val) = scope_entry.as_object().and_then(|o| o.get("@value")).and_then(|v| v.as_str()) {
                                scope_entries.push(ScopeHistoryEntry {
                                    time: Utc::now(),
                                    entity_id: entity_id_str.to_owned(),
                                    scope_path: scope_to_ltree(scope_val),
                                    instance_id: format!("urn:ngsi-ld:scope:instance:{}", uuid::Uuid::new_v4()),
                                });
                            }
                        }
                    }
                    continue;
                }

                let instances = match attr_value {
                    Value::Array(arr) => arr,
                    _other => continue,
                };

                for instance in instances {
                    let Some(inst_obj) = instance.as_object() else {
                        continue;
                    };
                    if !is_ngsi_ld_attribute_instance(inst_obj) {
                        continue;
                    }

                    let attribute_type = detect_attribute_type(instance);
                    let typed_value = match extract_typed_value(instance, attribute_type) {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::warn!(error = %e, entity_id = %entity_id_str, attr = %attr_name, event = "temporal_batch_extract_failed");
                            continue;
                        }
                    };
                    let value_type = typed_value.value_type();
                    let dataset_id = extract_dataset_id(instance);
                    let observed_at_str = extract_observed_at(instance);
                    let (instance_id, instance_id_client_supplied) = extract_or_generate_instance_id(instance);
                    let metadata = collect_metadata(instance);

                    let time = observed_at_str
                        .as_ref()
                        .and_then(|s| DateTime::parse_from_rfc3339(s).map(|dt| dt.with_timezone(&Utc)).ok())
                        .unwrap_or_else(Utc::now);
                    let observed_at = observed_at_str
                        .as_ref()
                        .and_then(|s| DateTime::parse_from_rfc3339(s).map(|dt| dt.with_timezone(&Utc)).ok());

                    let tea_idx = tea_entries.len();
                    tea_entries.push(TeaBatchEntry {
                        entity_id: entity_id_str.to_owned(),
                        attribute_name: attr_name.clone(),
                        attribute_type,
                        dataset_id: dataset_id.map(|u| u.to_string()),
                        value_type,
                    });
                    pending_histories.push(PendingHistory {
                        time,
                        db_params: typed_value.into_db_params(),
                        instance_id,
                        instance_id_client_supplied,
                        observed_at,
                        metadata,
                    });
                    tea_index.push(tea_idx);
                }
            }
        }

        PreparedBatch {
            tea_entries,
            pending_histories,
            tea_index,
            scope_entries,
        }
    }

    fn build_history_entries(prepared: &PreparedBatch, tea_results: &[TeaBatchResultRow]) -> Vec<HistoryBatchEntry> {
        let mut tea_id_map: std::collections::HashMap<(&str, &str, Option<&str>), uuid::Uuid> = std::collections::HashMap::with_capacity(tea_results.len());
        for r in tea_results {
            tea_id_map.insert((r.entity_id.as_str(), r.attribute_name.as_str(), r.dataset_id.as_deref()), r.id);
        }

        let mut history_entries = Vec::with_capacity(prepared.pending_histories.len());
        for (i, ph) in prepared.pending_histories.iter().enumerate() {
            let tea_entry = &prepared.tea_entries[prepared.tea_index[i]];
            let tea_id = match tea_id_map.get(&(tea_entry.entity_id.as_str(), tea_entry.attribute_name.as_str(), tea_entry.dataset_id.as_deref())) {
                Some(id) => *id,
                None => {
                    tracing::warn!(
                        entity_id = %tea_entry.entity_id,
                        attr = %tea_entry.attribute_name,
                        event = "tea_id_not_found_in_batch_result"
                    );
                    continue;
                }
            };
            history_entries.push(HistoryBatchEntry {
                time: ph.time,
                tea_id,
                value_number: ph.db_params.value_number,
                value_string: ph.db_params.value_string.clone(),
                value_bool: ph.db_params.value_bool,
                value_geo: ph.db_params.value_geo.clone(),
                value_object: ph.db_params.value_object.clone(),
                value_datetime: ph.db_params.value_datetime,
                value_uri: ph.db_params.value_uri.clone(),
                instance_id: ph.instance_id.clone(),
                observed_at: ph.observed_at,
                metadata: ph.metadata.clone(),
                instance_id_client_supplied: ph.instance_id_client_supplied,
            });
        }
        history_entries
    }

    /// Record a tombstone for a deleted attribute (soft-delete) inside the
    /// caller's transaction.
    ///
    /// Returns the single history row written so the caller can queue
    /// it with the unit-of-work for post-commit `attribute_current`
    /// materialisation.
    pub async fn record_attribute_deletion_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        entity_id: &str,
        attr_iri: &str,
        dataset_id: Option<&str>,
    ) -> Result<Vec<HistoryBatchEntry>, AndromedaError> {
        let (tea_entry, history_entry) = self.build_deletion_entries(entity_id, attr_iri, dataset_id).await?;
        let tea_results = self.repo.upsert_tea_batch_in_tx(tx, std::slice::from_ref(&tea_entry)).await?;
        let tea_id = first_tea_id(tea_results)?;
        let mut history = history_entry;
        history.tea_id = tea_id;
        self.repo.insert_history_batch_in_tx(tx, std::slice::from_ref(&history)).await?;
        Ok(vec![history])
    }

    async fn build_deletion_entries(
        &self,
        entity_id: &str,
        attr_iri: &str,
        dataset_id: Option<&str>,
    ) -> Result<(TeaBatchEntry, HistoryBatchEntry), AndromedaError> {
        let teas = self.repo.get_teas_for_entity_attr(entity_id, attr_iri).await?;
        let parsed_dataset_id = dataset_id.map(Urn::try_from).transpose()?;
        let attr_type = teas
            .iter()
            .find(|t| t.dataset_id.as_ref() == parsed_dataset_id.as_ref())
            .or_else(|| teas.first())
            .map_or(AttributeType::Property, |t| t.attribute_type);

        let dataset_id_str = parsed_dataset_id.as_ref().map(ToString::to_string);
        let tea_entry = TeaBatchEntry {
            entity_id: entity_id.to_owned(),
            attribute_name: attr_iri.to_owned(),
            attribute_type: attr_type,
            dataset_id: dataset_id_str,
            value_type: ValueType::String,
        };
        let history_entry = HistoryBatchEntry {
            time: Utc::now(),
            tea_id: uuid::Uuid::nil(),
            value_number: None,
            value_string: Some(vocab::expanded::properties::NULL_URI.as_str().to_owned()),
            value_bool: None,
            value_geo: None,
            value_object: None,
            value_datetime: None,
            value_uri: None,
            instance_id: format!("urn:ngsi-ld:attribute:instance:{}", uuid::Uuid::new_v4()),
            observed_at: None,
            metadata: None,
            instance_id_client_supplied: false,
        };
        Ok((tea_entry, history_entry))
    }

    /// Update an existing attribute instance (full replacement, preserves createdAt).
    pub async fn update_attribute_instance(&self, entity_id: &str, attr_name: &str, instance_id: &str, instance: &Value) -> Result<(), AndromedaError> {
        let (params_owner, observed_at) = build_update_instance_state(instance)?;
        let params = build_update_instance_params(instance_id, entity_id, attr_name, &params_owner, observed_at);
        self.repo.update_instance(params).await
    }
}

struct UpdateInstanceOwned {
    db_params: DbValueParams,
    metadata: Option<Value>,
}

fn build_update_instance_state(instance: &Value) -> Result<(UpdateInstanceOwned, Option<DateTime<Utc>>), AndromedaError> {
    let attribute_type = detect_attribute_type(instance);
    let typed_value = extract_typed_value(instance, attribute_type)?;
    let observed_at = extract_observed_at(instance).and_then(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)).ok());
    let metadata = collect_metadata(instance);
    let db_params = typed_value.into_db_params();
    Ok((UpdateInstanceOwned { db_params, metadata }, observed_at))
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
            value_number: owner.db_params.value_number,
            value_string: owner.db_params.value_string.as_deref(),
            value_bool: owner.db_params.value_bool,
            value_geo: owner.db_params.value_geo.as_ref(),
            value_object: owner.db_params.value_object.as_ref(),
            value_datetime: owner.db_params.value_datetime,
            value_uri: owner.db_params.value_uri.as_deref(),
            metadata: owner.metadata.as_ref(),
        },
        observed_at,
    }
}

fn first_tea_id(tea_results: Vec<TeaBatchResultRow>) -> Result<uuid::Uuid, AndromedaError> {
    tea_results
        .into_iter()
        .next()
        .map(|r| r.id)
        .ok_or_else(|| AndromedaError::internal("upsert_tea_batch returned no rows".to_string()))
}

// -----------------------------------------------------------------------------
// JSON-LD value extraction (shared with TemporalService)
// -----------------------------------------------------------------------------

/// Check if an expanded JSON-LD object is a recognized NGSI-LD attribute instance
/// (Property, Relationship, GeoProperty, VocabProperty, LanguageProperty, or JsonProperty).
fn is_ngsi_ld_attribute_instance(obj: &serde_json::Map<String, Value>) -> bool {
    let Some(types) = obj.get("@type").and_then(|v| v.as_array()) else {
        return false;
    };
    types.iter().filter_map(|t| t.as_str()).any(|s| AttributeType::from_expanded_iri(s).is_some())
}

fn detect_attribute_type(instance: &Value) -> AttributeType {
    let empty: Vec<serde_json::Value> = vec![];
    let types = instance.get("@type").and_then(|v| v.as_array()).unwrap_or(&empty);

    for t in types {
        if let Some(s) = t.as_str()
            && let Some(attr_type) = AttributeType::from_expanded_iri(s)
        {
            return attr_type;
        }
    }
    AttributeType::Property
}

#[derive(Debug)]
enum TypedValue {
    Number(f64),
    String(String),
    Boolean(bool),
    Geometry(Wkb),
    Object(Value),
    DateTime(String),
    Uri(String),
}

impl TypedValue {
    const fn value_type(&self) -> ValueType {
        match self {
            Self::Number(_) => ValueType::Number,
            Self::String(_) => ValueType::String,
            Self::Boolean(_) => ValueType::Boolean,
            Self::Geometry(_) => ValueType::Geometry,
            Self::Object(_) => ValueType::Object,
            Self::DateTime(_) => ValueType::DateTime,
            Self::Uri(_) => ValueType::Uri,
        }
    }

    fn into_db_params(self) -> DbValueParams {
        let mut p = DbValueParams {
            value_number: None,
            value_string: None,
            value_bool: None,
            value_geo: None,
            value_object: None,
            value_datetime: None,
            value_uri: None,
        };
        match self {
            Self::Number(n) => p.value_number = Some(n),
            Self::String(s) => p.value_string = Some(s),
            Self::Boolean(b) => p.value_bool = Some(b),
            Self::Geometry(g) => p.value_geo = Some(g),
            Self::Object(o) => p.value_object = Some(o),
            Self::DateTime(s) => {
                p.value_datetime = DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)).ok();
            }
            Self::Uri(u) => p.value_uri = Some(u),
        }
        p
    }
}

fn extract_typed_value(instance: &Value, attribute_type: AttributeType) -> Result<TypedValue, AndromedaError> {
    match attribute_type {
        AttributeType::Relationship => {
            let uri = instance
                .get(vocab::expanded::properties::OBJECT.as_str())
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|obj| obj.get("@id"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| AndromedaError::temporal_validation("Relationship missing object"))?;
            Ok(TypedValue::Uri(uri.to_string()))
        }
        AttributeType::GeoProperty => {
            let geo_node = instance
                .get(vocab::expanded::properties::VALUE.as_str())
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .ok_or_else(|| AndromedaError::temporal_validation("GeoProperty missing value"))?;

            // Literal-`@value` form (string or already-compact object) keeps the
            // legacy decoding path. Expanded JSON-LD GeoJSON nodes go through the
            // direct walker, skipping the compact-string round-trip and
            // `extract_coordinates` clones that dominated the temporal hot path.
            let wkb = if let Some(val) = geo_node.get("@value") {
                let geo_value: Value = match val.as_str() {
                    Some(s) => serde_json::from_str(s)
                        .map_err(|error| AndromedaError::temporal_validation(format!("Invalid GeoProperty GeoJSON string: {}", error)))?,
                    None => val.clone(), // clone: need owned Value for from_geojson_value
                };
                Wkb::from_geojson_value(&geo_value).map_err(|error| AndromedaError::temporal_validation(format!("Invalid GeoProperty geometry: {}", error)))?
            } else {
                expanded_geo_node_to_wkb(geo_node).ok_or_else(|| AndromedaError::temporal_validation("Invalid expanded GeoProperty geometry"))?
            };
            Ok(TypedValue::Geometry(wkb))
        }
        AttributeType::VocabProperty => {
            let arr = instance
                .get(vocab::expanded::properties::VOCAB.as_str())
                .and_then(|v| v.as_array())
                .ok_or_else(|| AndromedaError::temporal_validation("VocabProperty missing vocab value"))?;
            if arr.len() > 1 {
                let values: Vec<Value> = arr.iter().filter_map(|obj| obj.get("@id").or(obj.get("@value")).cloned()).collect(); // clone: need owned Values for collected Vec
                Ok(TypedValue::Object(Value::Array(values)))
            } else {
                let vocab_val = arr
                    .first()
                    .and_then(|obj| obj.get("@id").or(obj.get("@value")))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AndromedaError::temporal_validation("VocabProperty missing vocab value"))?;
                Ok(TypedValue::String(vocab_val.to_string()))
            }
        }
        AttributeType::LanguageProperty => {
            let lang_map = instance
                .get(vocab::expanded::properties::LANGUAGE_MAP.as_str())
                .ok_or_else(|| AndromedaError::temporal_validation("LanguageProperty missing languageMap"))?;
            Ok(TypedValue::Object(lang_map.clone())) // clone: need owned Value for TypedValue enum; lang_map is borrowed from instance
        }
        AttributeType::JsonProperty => {
            let json_val = instance
                .get(vocab::expanded::properties::JSON.as_str())
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|obj| obj.get("@value"))
                .ok_or_else(|| AndromedaError::temporal_validation("JsonProperty missing json value"))?;
            Ok(TypedValue::Object(json_val.clone())) // clone: need owned Value for TypedValue enum; json_val is borrowed from instance
        }
        AttributeType::ListProperty => {
            let list = instance
                .get(vocab::expanded::properties::VALUE_LIST.as_str())
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|obj| obj.get("@list"))
                .ok_or_else(|| AndromedaError::temporal_validation("ListProperty missing valueList"))?;
            Ok(TypedValue::Object(list.clone())) // clone: need owned Value for TypedValue enum; list is borrowed from instance
        }
        AttributeType::ListRelationship => {
            let list = instance
                .get(vocab::expanded::properties::OBJECT_LIST.as_str())
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|obj| obj.get("@list"))
                .ok_or_else(|| AndromedaError::temporal_validation("ListRelationship missing objectList"))?;
            let uris: Vec<Value> = list
                .as_array()
                .map(|arr| arr.iter().filter_map(|obj| obj.get("@id").cloned()).collect()) // clone: need owned Values for collected Vec
                .unwrap_or_default();
            Ok(TypedValue::Object(Value::Array(uris)))
        }
        AttributeType::Property => {
            // Property -- uses hasValue
            let has_value = instance
                .get(vocab::expanded::properties::VALUE.as_str())
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .ok_or_else(|| AndromedaError::temporal_validation("Property missing value"))?;

            if let Some(value) = has_value.get("@value") {
                let type_hint = has_value.get("@type").and_then(|t| t.as_str());

                if (type_hint == Some(vocab::expanded::classes::DATE_TIME.as_str()) || type_hint == Some(vocab::expanded::properties::XSD_DATETIME.as_str()))
                    && let Some(s) = value.as_str()
                {
                    return Ok(TypedValue::DateTime(s.to_string()));
                }

                match value {
                    Value::Number(n) => Ok(TypedValue::Number(n.as_f64().unwrap_or(0.0))),
                    Value::Bool(b) => Ok(TypedValue::Boolean(*b)),
                    Value::String(s) => Ok(TypedValue::String(s.clone())), // clone: need owned String for TypedValue enum; s is borrowed from JSON
                    Value::Object(_) | Value::Array(_) => Ok(TypedValue::Object(value.clone())), // clone: need owned Value for TypedValue enum; value is borrowed from JSON
                    Value::Null => Ok(TypedValue::String("null".to_string())),
                }
            } else {
                // Expanded JSON-LD node without @value -- store as Object
                Ok(TypedValue::Object(has_value.clone())) // clone: need owned Value for TypedValue enum; has_value is borrowed from instance
            }
        }
    }
}

fn extract_observed_at(instance: &Value) -> Option<String> {
    instance
        .get(vocab::expanded::properties::OBSERVED_AT.as_str())
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|obj| obj.get("@value"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn extract_dataset_id(instance: &Value) -> Option<Urn> {
    instance
        .get(vocab::expanded::properties::DATASET_ID.as_str())
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|obj| obj.get("@id"))
        .and_then(|v| v.as_str())
        .and_then(|value| Urn::try_from(value).ok())
}

fn extract_or_generate_instance_id(instance: &Value) -> (String, bool) {
    instance
        .get(vocab::expanded::properties::INSTANCE_ID.as_str())
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|obj| obj.get("@id"))
        .and_then(|v| v.as_str())
        .map_or_else(
            || (format!("urn:ngsi-ld:attribute:instance:{}", uuid::Uuid::new_v4()), false),
            |s| (s.to_owned(), true),
        )
}

/// Returns whether the metadata key is stored in dedicated columns instead.
fn is_metadata_skip_key(key: &str) -> bool {
    key == "@id"
        || key == "@type"
        || key == vocab::expanded::properties::DATASET_ID.as_str()
        || key == vocab::expanded::properties::JSON.as_str()
        || key == vocab::expanded::properties::LANGUAGE_MAP.as_str()
        || key == vocab::expanded::properties::OBJECT.as_str()
        || key == vocab::expanded::properties::VALUE.as_str()
        || key == vocab::expanded::properties::VOCAB.as_str()
        || key == vocab::expanded::properties::INSTANCE_ID.as_str()
        || key == vocab::expanded::properties::OBSERVED_AT.as_str()
}

fn collect_metadata(instance: &Value) -> Option<Value> {
    let obj = instance.as_object()?;

    let metadata: serde_json::Map<String, Value> = obj
        .iter()
        .filter(|(k, _)| !is_metadata_skip_key(k))
        .map(|(k, v)| (k.clone(), v.clone())) // clone: owned key-value pairs for new metadata Map
        .collect();

    if metadata.is_empty() { None } else { Some(Value::Object(metadata)) }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    #[test]
    fn metadata_skip_key_matches_known_values() {
        assert!(is_metadata_skip_key("@id"));
        assert!(is_metadata_skip_key(vocab::expanded::properties::DATASET_ID.as_str()));
        assert!(is_metadata_skip_key(vocab::expanded::properties::VALUE.as_str()));
        assert!(!is_metadata_skip_key("https://example.com/custom"));
    }

    #[test]
    fn system_attribute_matches_known_values() {
        assert!(is_system_attribute(vocab::expanded::properties::CREATED_AT.as_str()));
        assert!(is_system_attribute(vocab::expanded::properties::UNIT_CODE.as_str()));
        assert!(!is_system_attribute("https://example.com/custom"));
    }

    #[test]
    fn extract_instance_id_flags_client_supplied_when_present() {
        let instance = serde_json::json!({
            vocab::expanded::properties::INSTANCE_ID.as_str(): [
                {"@id": "urn:ngsi-ld:attribute:instance:provided"}
            ]
        });
        let (id, supplied) = extract_or_generate_instance_id(&instance);
        assert_eq!(id, "urn:ngsi-ld:attribute:instance:provided");
        assert!(supplied);
    }

    #[test]
    fn extract_instance_id_flags_generated_when_absent() {
        let instance = serde_json::json!({});
        let (id, supplied) = extract_or_generate_instance_id(&instance);
        assert!(id.starts_with("urn:ngsi-ld:attribute:instance:"));
        assert!(!supplied);
    }

    #[test]
    fn prepare_batch_refs_propagates_client_supplied_flag() {
        let attr = vocab::expanded::properties::VALUE.as_str();
        let property_type = AttributeType::Property.iri_str();
        let entity = serde_json::json!({
            "@id": "urn:ngsi-ld:Entity:e1",
            "@type": ["urn:ngsi-ld:Type:T"],
            "https://example.com/withId": [{
                "@type": [property_type],
                attr: [{"@value": 1}],
                vocab::expanded::properties::INSTANCE_ID.as_str(): [
                    {"@id": "urn:ngsi-ld:attribute:instance:client"}
                ]
            }],
            "https://example.com/noId": [{
                "@type": [property_type],
                attr: [{"@value": 2}]
            }],
        });
        let entities = [("urn:ngsi-ld:Entity:e1", &entity)];
        let prepared = TemporalRecorder::prepare_batch_refs(&entities);
        assert_eq!(prepared.pending_histories.len(), 2);
        let with_id = prepared
            .pending_histories
            .iter()
            .find(|p| p.instance_id == "urn:ngsi-ld:attribute:instance:client")
            .expect("client-supplied entry");
        let without_id = prepared
            .pending_histories
            .iter()
            .find(|p| p.instance_id != "urn:ngsi-ld:attribute:instance:client")
            .expect("generated entry");
        assert!(with_id.instance_id_client_supplied);
        assert!(!without_id.instance_id_client_supplied);
    }

    #[test]
    fn build_history_entries_carries_flag_to_history_batch_entry() {
        let attr = vocab::expanded::properties::VALUE.as_str();
        let property_type = AttributeType::Property.iri_str();
        let entity = serde_json::json!({
            "@id": "urn:ngsi-ld:Entity:e1",
            "@type": ["urn:ngsi-ld:Type:T"],
            "https://example.com/withId": [{
                "@type": [property_type],
                attr: [{"@value": 1}],
                vocab::expanded::properties::INSTANCE_ID.as_str(): [
                    {"@id": "urn:ngsi-ld:attribute:instance:client"}
                ]
            }],
        });
        let entities = [("urn:ngsi-ld:Entity:e1", &entity)];
        let prepared = TemporalRecorder::prepare_batch_refs(&entities);
        let tea_results: Vec<TeaBatchResultRow> = prepared
            .tea_entries
            .iter()
            .map(|t| TeaBatchResultRow {
                id: uuid::Uuid::new_v4(),
                entity_id: t.entity_id.clone(),
                attribute_name: t.attribute_name.clone(),
                dataset_id: t.dataset_id.clone(),
            })
            .collect();
        let history = TemporalRecorder::build_history_entries(&prepared, &tea_results);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].instance_id, "urn:ngsi-ld:attribute:instance:client");
        assert!(history[0].instance_id_client_supplied);
    }
}
