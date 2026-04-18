//! Shared temporal write-path logic for Entity API and Temporal API.
//!
//! Records attribute history whenever entities are created or modified.

use crate::{
    core::error::AndromedaError,
    domain::{attribute_type::AttributeType, temporal::ValueType},
    jsonld::vocab,
    query::sql::scope_to_ltree,
    repository::temporal::{
        params::{HistoryBatchEntry, HistoryValues, ScopeHistoryEntry, TeaBatchEntry, UpdateInstanceParams},
        repo::TemporalRepository,
    },
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;
use urn::Urn;

/// Returns whether the given expanded IRI is an NGSI-LD system attribute.
fn is_system_attribute(attr_name: &str) -> bool {
    matches!(
        attr_name,
        value if value == vocab::expanded::CREATED_AT.as_str()
            || value == vocab::expanded::DATASET_ID.as_str()
            || value == vocab::expanded::DELETED_AT.as_str()
            || value == vocab::expanded::INSTANCE_ID.as_str()
            || value == vocab::expanded::MODIFIED_AT.as_str()
            || value == vocab::expanded::OBSERVED_AT.as_str()
            || value == vocab::expanded::UNIT_CODE.as_str()
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
    /// GeoJSON string for geo-typed properties.
    pub value_geo: Option<String>,
    /// Arbitrary JSON value for object-typed or complex properties.
    pub value_object: Option<Value>,
    /// Datetime value for datetime-typed properties.
    pub value_datetime: Option<DateTime<Utc>>,
    /// URI string for relationship objects.
    pub value_uri: Option<String>,
}

/// Records attribute history into temporal tables when entities are created or modified.
pub struct TemporalRecorder {
    repo: Arc<TemporalRepository>,
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
            if attr_name == vocab::expanded::SCOPE.as_str() {
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
        let instance_id = extract_or_generate_instance_id(instance);
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
    pub async fn record_entities_batch(&self, entities: &[(String, Value)]) -> Result<(), AndromedaError> {
        if entities.is_empty() {
            return Ok(());
        }

        // Phase 1: Collect all TEA entries from all entities.
        // Each entry is paired with its "pending history" data so we can
        // build history rows once we have the TEA UUIDs.
        struct PendingHistory {
            time: DateTime<Utc>,
            db_params: DbValueParams,
            instance_id: String,
            observed_at: Option<DateTime<Utc>>,
            metadata: Option<Value>,
        }

        let mut tea_entries = Vec::new();
        let mut pending_histories: Vec<PendingHistory> = Vec::new();
        // tea_index[i] corresponds to pending_histories[i] and maps to tea_entries index
        let mut tea_index: Vec<usize> = Vec::new();
        // Scope entries collected separately -- bypass TEA/attribute_history pipeline.
        let mut scope_entries: Vec<ScopeHistoryEntry> = Vec::new();

        for (entity_id, expanded) in entities {
            let obj = match expanded {
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

                // Handle scope entries: write directly to scope_history, not attribute_history.
                if attr_name == vocab::expanded::SCOPE.as_str() {
                    if let Some(scope_arr) = attr_value.as_array() {
                        for scope_entry in scope_arr {
                            if let Some(scope_val) = scope_entry.as_object().and_then(|o| o.get("@value")).and_then(|v| v.as_str()) {
                                scope_entries.push(ScopeHistoryEntry {
                                    time: Utc::now(),
                                    entity_id: entity_id.clone(), // clone: owned String for batch entry
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
                            tracing::warn!(error = %e, entity_id = %entity_id, attr = %attr_name, event = "temporal_batch_extract_failed");
                            continue;
                        }
                    };
                    let value_type = typed_value.value_type();
                    let dataset_id = extract_dataset_id(instance);
                    let observed_at_str = extract_observed_at(instance);
                    let instance_id = extract_or_generate_instance_id(instance);
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
                        entity_id: entity_id.clone(),      // clone: owned String for batch entry
                        attribute_name: attr_name.clone(), // clone: owned String for batch entry
                        attribute_type,
                        dataset_id: dataset_id.map(|u| u.to_string()),
                        value_type,
                    });
                    pending_histories.push(PendingHistory {
                        time,
                        db_params: typed_value.into_db_params(),
                        instance_id,
                        observed_at,
                        metadata,
                    });
                    tea_index.push(tea_idx);
                }
            }
        }

        if tea_entries.is_empty() {
            return Ok(());
        }

        // Phase 2: Bulk upsert all TEA entries in a single SQL round-trip.
        let tea_results = self.repo.upsert_tea_batch(&tea_entries).await?;

        // Build a lookup map: (entity_id, attribute_name, dataset_id) -> tea_id
        let mut tea_id_map: std::collections::HashMap<(&str, &str, Option<&str>), uuid::Uuid> = std::collections::HashMap::with_capacity(tea_results.len());
        for r in &tea_results {
            tea_id_map.insert((r.entity_id.as_str(), r.attribute_name.as_str(), r.dataset_id.as_deref()), r.id);
        }

        // Phase 3: Build history entries by resolving TEA UUIDs from the map.
        let mut history_entries = Vec::with_capacity(pending_histories.len());
        for (i, ph) in pending_histories.into_iter().enumerate() {
            let tea_entry = &tea_entries[tea_index[i]];
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
                value_string: ph.db_params.value_string,
                value_bool: ph.db_params.value_bool,
                value_geo: ph.db_params.value_geo,
                value_object: ph.db_params.value_object,
                value_datetime: ph.db_params.value_datetime,
                value_uri: ph.db_params.value_uri,
                instance_id: ph.instance_id,
                observed_at: ph.observed_at,
                metadata: ph.metadata,
            });
        }

        // Phase 4: Bulk insert all history rows in a single transaction.
        self.repo.insert_history_batch(&history_entries).await?;

        // Phase 5: Bulk insert scope history entries (separate from attribute pipeline).
        if !scope_entries.is_empty() {
            self.repo.insert_scope_history_batch(&scope_entries).await?;
        }

        Ok(())
    }

    /// Record a tombstone for a deleted attribute (soft-delete).
    pub async fn record_attribute_deletion(&self, entity_id: &str, attr_iri: &str, dataset_id: Option<&str>) -> Result<(), AndromedaError> {
        // Look up the existing TEA to preserve the attribute type
        let teas = self.repo.get_teas_for_entity_attr(entity_id, attr_iri).await?;
        let parsed_dataset_id = dataset_id.map(Urn::try_from).transpose()?;
        let attr_type = teas
            .iter()
            .find(|t| t.dataset_id.as_ref() == parsed_dataset_id.as_ref())
            .or_else(|| teas.first())
            .map_or(AttributeType::Property, |t| t.attribute_type);

        let dataset_id_str = parsed_dataset_id.as_ref().map(ToString::to_string);
        let tea_results = self
            .repo
            .upsert_tea_batch(&[TeaBatchEntry {
                entity_id: entity_id.to_owned(),
                attribute_name: attr_iri.to_owned(),
                attribute_type: attr_type,
                dataset_id: dataset_id_str,
                value_type: ValueType::String,
            }])
            .await?;
        let tea_id = tea_results
            .into_iter()
            .next()
            .ok_or_else(|| AndromedaError::internal("upsert_tea_batch returned no rows for deletion tombstone".to_string()))?
            .id;

        let now = Utc::now();
        let instance_id = format!("urn:ngsi-ld:attribute:instance:{}", uuid::Uuid::new_v4());

        self.repo
            .insert_history_batch(&[HistoryBatchEntry {
                time: now,
                tea_id,
                value_number: None,
                value_string: Some(vocab::expanded::NULL_URI.as_str().to_owned()),
                value_bool: None,
                value_geo: None,
                value_object: None,
                value_datetime: None,
                value_uri: None,
                instance_id,
                observed_at: None,
                metadata: None,
            }])
            .await?;

        Ok(())
    }

    /// Update an existing attribute instance (full replacement, preserves createdAt).
    pub async fn update_attribute_instance(&self, entity_id: &str, attr_name: &str, instance_id: &str, instance: &Value) -> Result<(), AndromedaError> {
        let attribute_type = detect_attribute_type(instance);
        let typed_value = extract_typed_value(instance, attribute_type)?;
        let observed_at = extract_observed_at(instance).and_then(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)).ok());
        let metadata = collect_metadata(instance);

        let p = typed_value.into_db_params();

        self.repo
            .update_instance(UpdateInstanceParams {
                instance_id,
                entity_id,
                attribute_name: attr_name,
                values: HistoryValues {
                    value_number: p.value_number,
                    value_string: p.value_string.as_deref(),
                    value_bool: p.value_bool,
                    value_geo: p.value_geo.as_deref(),
                    value_object: p.value_object.as_ref(),
                    value_datetime: p.value_datetime,
                    value_uri: p.value_uri.as_deref(),
                    metadata: metadata.as_ref(),
                },
                observed_at,
            })
            .await
    }
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
    Geometry(String),
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
                .get(vocab::expanded::OBJECT.as_str())
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|obj| obj.get("@id"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| AndromedaError::temporal_validation("Relationship missing object"))?;
            Ok(TypedValue::Uri(uri.to_string()))
        }
        AttributeType::GeoProperty => {
            let geo_node = instance
                .get(vocab::expanded::VALUE.as_str())
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .ok_or_else(|| AndromedaError::temporal_validation("GeoProperty missing value"))?;

            let geo_str = if let Some(val) = geo_node.get("@value") {
                match val.as_str() {
                    Some(s) => s.to_string(),
                    None => val.to_string(),
                }
            } else {
                reconstruct_geojson(geo_node)
            };
            Ok(TypedValue::Geometry(geo_str))
        }
        AttributeType::VocabProperty => {
            let arr = instance
                .get(vocab::expanded::VOCAB.as_str())
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
                .get(vocab::expanded::LANGUAGE_MAP.as_str())
                .ok_or_else(|| AndromedaError::temporal_validation("LanguageProperty missing languageMap"))?;
            Ok(TypedValue::Object(lang_map.clone())) // clone: need owned Value for TypedValue enum; lang_map is borrowed from instance
        }
        AttributeType::JsonProperty => {
            let json_val = instance
                .get(vocab::expanded::JSON.as_str())
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|obj| obj.get("@value"))
                .ok_or_else(|| AndromedaError::temporal_validation("JsonProperty missing json value"))?;
            Ok(TypedValue::Object(json_val.clone())) // clone: need owned Value for TypedValue enum; json_val is borrowed from instance
        }
        AttributeType::ListProperty => {
            let list = instance
                .get(vocab::expanded::VALUE_LIST.as_str())
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|obj| obj.get("@list"))
                .ok_or_else(|| AndromedaError::temporal_validation("ListProperty missing valueList"))?;
            Ok(TypedValue::Object(list.clone())) // clone: need owned Value for TypedValue enum; list is borrowed from instance
        }
        AttributeType::ListRelationship => {
            let list = instance
                .get(vocab::expanded::OBJECT_LIST.as_str())
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
                .get(vocab::expanded::VALUE.as_str())
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .ok_or_else(|| AndromedaError::temporal_validation("Property missing value"))?;

            if let Some(value) = has_value.get("@value") {
                let type_hint = has_value.get("@type").and_then(|t| t.as_str());

                if (type_hint == Some(vocab::expanded::DATE_TIME.as_str()) || type_hint == Some(vocab::expanded::XSD_DATETIME.as_str()))
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

/// Reconstruct compact GeoJSON from an expanded JSON-LD GeoJSON node.
///
/// Expanded GeoJSON has structure like:
/// ```json
/// {
///   "@type": ["https://purl.org/geojson/vocab#Point"],
///   "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": 16.37}, {"@value": 48.20}]}]
/// }
/// ```
/// We reconstruct: `{"type": "Point", "coordinates": [16.37, 48.20]}`
pub(crate) fn reconstruct_geojson(node: &Value) -> String {
    let Some(obj) = node.as_object() else {
        return node.to_string();
    };

    // Extract the geometry type from @type
    let geo_type = obj
        .get("@type")
        .and_then(|t| t.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .and_then(|s| s.strip_prefix(vocab::prefix::GEOJSON.as_str()))
        .unwrap_or("Point");

    // Extract coordinates from the expanded form
    let coordinates = obj
        .get(vocab::expanded::COORDINATES.as_str())
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("@list").or(Some(v)))
        .map_or(Value::Array(vec![]), extract_coordinates);

    let result = serde_json::json!({
        "type": geo_type,
        "coordinates": coordinates
    });
    result.to_string()
}

/// Recursively extract coordinate values from expanded JSON-LD @list structures.
pub(crate) fn extract_coordinates(value: &Value) -> Value {
    match value {
        Value::Array(arr) => {
            let items: Vec<Value> = arr.iter().map(extract_coordinates).collect();
            Value::Array(items)
        }
        Value::Object(obj) => {
            if let Some(val) = obj.get("@value") {
                val.clone() // clone: need owned Value for recursive coordinate extraction
            } else if let Some(list) = obj.get("@list") {
                extract_coordinates(list)
            } else {
                value.clone() // clone: need owned Value; unrecognized object passed through as-is
            }
        }
        _other => value.clone(), // clone: need owned Value for return; primitives don't implement Copy for serde_json::Value
    }
}

fn extract_observed_at(instance: &Value) -> Option<String> {
    instance
        .get(vocab::expanded::OBSERVED_AT.as_str())
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|obj| obj.get("@value"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn extract_dataset_id(instance: &Value) -> Option<Urn> {
    instance
        .get(vocab::expanded::DATASET_ID.as_str())
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|obj| obj.get("@id"))
        .and_then(|v| v.as_str())
        .and_then(|value| Urn::try_from(value).ok())
}

fn extract_or_generate_instance_id(instance: &Value) -> String {
    instance
        .get(vocab::expanded::INSTANCE_ID.as_str())
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|obj| obj.get("@id"))
        .and_then(|v| v.as_str())
        .map_or_else(|| format!("urn:ngsi-ld:attribute:instance:{}", uuid::Uuid::new_v4()), String::from)
}

/// Returns whether the metadata key is stored in dedicated columns instead.
fn is_metadata_skip_key(key: &str) -> bool {
    key == "@id"
        || key == "@type"
        || key == vocab::expanded::DATASET_ID.as_str()
        || key == vocab::expanded::JSON.as_str()
        || key == vocab::expanded::LANGUAGE_MAP.as_str()
        || key == vocab::expanded::OBJECT.as_str()
        || key == vocab::expanded::VALUE.as_str()
        || key == vocab::expanded::VOCAB.as_str()
        || key == vocab::expanded::INSTANCE_ID.as_str()
        || key == vocab::expanded::OBSERVED_AT.as_str()
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
        assert!(is_metadata_skip_key(vocab::expanded::DATASET_ID.as_str()));
        assert!(is_metadata_skip_key(vocab::expanded::VALUE.as_str()));
        assert!(!is_metadata_skip_key("https://example.com/custom"));
    }

    #[test]
    fn system_attribute_matches_known_values() {
        assert!(is_system_attribute(vocab::expanded::CREATED_AT.as_str()));
        assert!(is_system_attribute(vocab::expanded::UNIT_CODE.as_str()));
        assert!(!is_system_attribute("https://example.com/custom"));
    }
}
