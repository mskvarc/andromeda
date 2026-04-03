mod attrs;
mod batch;
mod discovery;
mod query;

pub use discovery::EntityDiscoveryService;
pub use query::EntityQueryService;

use crate::{
    domain::{
        entity::{Entity, EntityPatch},
        notification::{ChangeEvent, TriggerReason},
    },
    error::AndromedaError,
    jsonld::{ContextSpec, JsonLdEngine},
    repository::{entity::EntityRepository, temporal::TemporalRepository},
    service::temporal_recorder::TemporalRecorder,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Map, Value};
use sqlx::postgres::PgPool;
use std::{collections::HashSet, sync::Arc};
use tokio::sync::broadcast;
use tracing::debug;
use urn::Urn;

pub struct EntityService {
    repo: EntityRepository,
    temporal_recorder: TemporalRecorder,
    engine: JsonLdEngine,
    change_tx: Option<broadcast::Sender<ChangeEvent>>,
    batch_concurrency: usize,
}

impl EntityService {
    pub fn new(pool: &PgPool, engine: JsonLdEngine, change_tx: Option<broadcast::Sender<ChangeEvent>>, batch_concurrency: usize) -> EntityService {
        let repo = EntityRepository::new(pool);
        let temporal_repo = Arc::new(TemporalRepository::new(pool));
        let temporal_recorder = TemporalRecorder::new(temporal_repo);

        debug!("Entity service created!");

        EntityService {
            repo,
            temporal_recorder,
            engine,
            change_tx,
            batch_concurrency: batch_concurrency.max(1),
        }
    }

    fn emit_change(&self, event: ChangeEvent) {
        if let Some(ref tx) = self.change_tx {
            let _ = tx.send(event);
        }
    }

    fn emit_attr_change(&self, entity_id: String, entity_types: Vec<String>, trigger: TriggerReason, changed_attributes: Vec<String>, previous: Option<Value>) {
        self.emit_change(
            ChangeEvent::new(entity_id, entity_types, trigger)
                .with_changed_attributes(changed_attributes)
                .with_optional_previous(previous),
        );
    }

    async fn fetch_entity_types(&self, id: &str) -> Vec<String> {
        match self.repo.get(id).await {
            Ok(Some(stored)) => extract_entity_types(&stored),
            _ => vec![],
        }
    }

    // -------------------------------------------------------------------------
    // CRUD (write operations)
    // -------------------------------------------------------------------------

    pub async fn create(&self, entity: &Entity, context: &ContextSpec) -> Result<Urn, AndromedaError> {
        let id = entity.id.to_string();

        let mut entity_json = serde_json::to_value(entity).map_err(|err| AndromedaError::bad_request_data(format!("Invalid entity payload: {}", err)))?;
        normalize_entity_attributes(&mut entity_json);
        let expanded = self.engine.expand(entity_json, context).await?;
        validate_attribute_envelopes(&expanded)?;

        let types = extract_entity_types(&expanded);
        let types_refs: Vec<&str> = types.iter().map(String::as_str).collect();
        let scopes = extract_scopes(&expanded);

        let expires_at = crate::repository::entity::extract_expires_at(&expanded);
        let urn = self.repo.create(&id, types_refs.as_ref(), &scopes, &expanded, expires_at).await?;
        if let Err(e) = self.temporal_recorder.record_entity_attributes(&id, &expanded).await {
            tracing::warn!(error = %e, entity_id = %id, event = "temporal_recording_failed");
        }

        self.emit_change(ChangeEvent::new(id, types, TriggerReason::EntityCreated).with_changed_attributes(extract_attribute_keys(&expanded)));

        Ok(urn)
    }

    pub async fn merge(&self, id: &Urn, patch: &EntityPatch, context: &ContextSpec) -> Result<(), AndromedaError> {
        let id = id.to_string();

        let mut existing = self.repo.get(&id).await?.ok_or_else(|| AndromedaError::not_found(&id))?;
        let attrs_before: HashSet<String> = extract_attribute_keys(&existing).into_iter().collect();
        let previous_attributes = self.change_tx.as_ref().map(|_| existing.clone());

        let mut patch_json = serde_json::to_value(patch).map_err(|err| AndromedaError::bad_request_data(format!("Invalid patch payload: {}", err)))?;
        normalize_entity_attributes(&mut patch_json);
        let expanded = self.engine.expand(patch_json, context).await?;
        validate_attribute_envelopes(&expanded)?;
        crate::domain::expanded::merge_entity(&mut existing, &expanded).map_err(AndromedaError::internal)?;
        self.repo.save(&id, &existing).await?;

        if let Err(e) = self.temporal_recorder.record_entity_attributes(&id, &expanded).await {
            tracing::warn!(error = %e, entity_id = %id, event = "temporal_recording_failed");
        }

        let types = self.fetch_entity_types(&id).await;

        let attrs_after: HashSet<String> = extract_attribute_keys(&existing).into_iter().collect();
        let deleted_attrs: Vec<String> = attrs_before.difference(&attrs_after).cloned().collect();

        // Record temporal tombstones for deleted attributes
        for deleted_attr in &deleted_attrs {
            let _ = self.temporal_recorder.record_attribute_deletion(&id, deleted_attr, None).await;
        }

        for deleted_attr in &deleted_attrs {
            self.emit_attr_change(
                id.clone(),
                types.clone(),
                TriggerReason::AttributeDeleted,
                vec![deleted_attr.clone()],
                previous_attributes.clone(),
            );
        }

        self.emit_attr_change(id, types, TriggerReason::EntityUpdated, extract_attribute_keys(&expanded), previous_attributes);

        Ok(())
    }

    pub async fn replace_entity(&self, id: &Urn, patch: &EntityPatch, context: &ContextSpec) -> Result<(), AndromedaError> {
        let id = id.to_string();

        if self.repo.get(&id).await?.is_none() {
            return Err(AndromedaError::not_found(id));
        }

        let mut patch_json = serde_json::to_value(patch).map_err(|err| AndromedaError::bad_request_data(format!("Invalid patch payload: {}", err)))?;
        normalize_entity_attributes(&mut patch_json);
        let expanded = self.engine.expand(patch_json, context).await?;
        validate_attribute_envelopes(&expanded)?;

        self.repo.save(&id, &expanded).await?;
        if let Err(e) = self.temporal_recorder.record_entity_attributes(&id, &expanded).await {
            tracing::warn!(error = %e, entity_id = %id, event = "temporal_recording_failed");
        }

        let types = self.fetch_entity_types(&id).await;
        self.emit_change(ChangeEvent::new(id, types, TriggerReason::EntityUpdated).with_changed_attributes(extract_attribute_keys(&expanded)));

        Ok(())
    }

    pub async fn delete(&self, id: &Urn) -> Result<(), AndromedaError> {
        let id = id.to_string();

        let entity_snapshot = match self.repo.get(&id).await? {
            Some(expanded) => expanded,
            None => return Err(AndromedaError::not_found(id)),
        };
        let entity_types = extract_entity_types(&entity_snapshot);

        // Capture sys_attrs before deletion so notifications can include them
        let sys_attrs_snapshot = match self.repo.get_sys_attrs(&id).await {
            Ok(attrs) => attrs,
            Err(e) => {
                tracing::warn!("Failed to fetch sys_attrs before delete for {}: {}", id, e);
                None
            }
        };

        if !self.repo.delete(&id).await? {
            return Err(AndromedaError::not_found(id));
        }

        // Emit AttributeDeleted for each attribute so attributeDeleted subscriptions fire
        let attr_keys = extract_attribute_keys(&entity_snapshot);
        if !attr_keys.is_empty() {
            self.emit_change(
                ChangeEvent::new(id.clone(), entity_types.clone(), TriggerReason::AttributeDeleted)
                    .with_changed_attributes(attr_keys)
                    .with_snapshot(entity_snapshot.clone())
                    .with_previous(entity_snapshot.clone())
                    .with_sys_attrs(sys_attrs_snapshot.clone()),
            );
        }

        self.emit_change(
            ChangeEvent::new(id, entity_types, TriggerReason::EntityDeleted)
                .with_snapshot(entity_snapshot.clone())
                .with_previous(entity_snapshot)
                .with_sys_attrs(sys_attrs_snapshot),
        );

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn exists(&self, id: &Urn) -> Result<bool, AndromedaError> {
        let id = id.to_string();
        self.repo.exists(&id).await
    }
}

// ---------------------------------------------------------------------------
// Shared helpers (used across submodules)
// ---------------------------------------------------------------------------

pub(crate) use crate::domain::expanded::extract_types as extract_entity_types;

pub(crate) fn extract_scopes(expanded: &Value) -> Vec<String> {
    let obj = match expanded {
        Value::Array(arr) => arr.first().and_then(|v| v.as_object()),
        Value::Object(obj) => Some(obj),
        _ => None,
    };

    let Some(obj) = obj else {
        return Vec::new();
    };

    const SCOPE_IRI: &str = "https://uri.etsi.org/ngsi-ld/scope";

    let Some(scope_value) = obj.get(SCOPE_IRI) else {
        return Vec::new();
    };

    match scope_value {
        Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_object().and_then(|obj| obj.get("@value")).and_then(|v| v.as_str()).map(String::from))
            .collect(),
        Value::String(s) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn extract_attribute_keys(expanded: &Value) -> Vec<String> {
    let obj = match expanded {
        Value::Array(arr) => arr.first().and_then(|v| v.as_object()),
        Value::Object(obj) => Some(obj),
        _ => None,
    };

    obj.map(|o| o.keys().filter(|k| !k.starts_with('@')).cloned().collect()).unwrap_or_default()
}

fn unwrap_expanded_ref(value: &Value) -> &Value {
    match value {
        Value::Array(arr) if !arr.is_empty() => &arr[0],
        other => other,
    }
}

fn normalize_entity_attributes(payload: &mut Value) {
    let Some(obj) = payload.as_object_mut() else {
        return;
    };

    for (key, value) in obj.iter_mut() {
        if matches!(key.as_str(), "id" | "type" | "scope" | "@context" | "@id" | "@type") {
            continue;
        }
        normalize_attribute_value(value);
    }
}

fn normalize_attribute_value(value: &mut Value) {
    match value {
        Value::Object(obj) if is_explicit_attribute_object(obj) => {
            normalize_explicit_attribute_object(obj);
        }
        Value::Array(items) if is_explicit_attribute_array(items) => {
            for item in items {
                if let Value::Object(obj) = item {
                    normalize_explicit_attribute_object(obj);
                }
            }
        }
        _ => {
            let raw = std::mem::take(value);
            *value = wrap_raw_as_property(raw);
        }
    }
}

fn is_explicit_attribute_array(items: &[Value]) -> bool {
    !items.is_empty() && items.iter().all(|item| item.as_object().is_some_and(is_explicit_attribute_object))
}

fn is_explicit_attribute_object(obj: &Map<String, Value>) -> bool {
    obj.get("type").and_then(Value::as_str).is_some_and(is_known_attribute_type)
}

fn is_known_attribute_type(attr_type: &str) -> bool {
    attr_type.parse::<crate::domain::attribute_type::AttributeType>().is_ok()
}

fn normalize_explicit_attribute_object(obj: &mut Map<String, Value>) {
    let Some(attr_type_str) = obj.get("type").and_then(Value::as_str) else {
        return;
    };
    let reserved = match attr_type_str.parse::<crate::domain::attribute_type::AttributeType>() {
        Ok(t) => t.reserved_fields(),
        Err(_) => return,
    };

    for (key, value) in obj.iter_mut() {
        if key == "type" || reserved.contains(&key.as_str()) {
            continue;
        }
        normalize_attribute_value(value);
    }
}

fn wrap_raw_as_property(raw: Value) -> Value {
    let mut property = Map::new();
    property.insert("type".to_string(), Value::String("Property".to_string()));
    property.insert("value".to_string(), raw);
    Value::Object(property)
}

pub(crate) fn decode_percent_encoded_uris(value: &mut Value) {
    match value {
        Value::Object(obj) => {
            if obj.get("type").and_then(|t| t.as_str()) == Some("Relationship")
                && let Some(Value::String(uri)) = obj.get_mut("object")
                && uri.contains('%')
                && let Ok(decoded) = percent_encoding::percent_decode_str(uri).decode_utf8()
            {
                *uri = decoded.into_owned();
            }
            for v in obj.values_mut() {
                decode_percent_encoded_uris(v);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                decode_percent_encoded_uris(v);
            }
        }
        _ => {}
    }
}

use crate::domain::iri;

fn validate_attribute_envelopes(expanded: &Value) -> Result<(), AndromedaError> {
    let entities = match expanded {
        Value::Array(arr) => arr.as_slice(),
        _ => std::slice::from_ref(expanded),
    };

    for entity in entities {
        let Some(obj) = entity.as_object() else {
            continue;
        };
        for (key, value) in obj {
            if key.starts_with('@') {
                continue;
            }
            validate_attr_instances(key, value)?;
        }
    }
    Ok(())
}

fn validate_attr_instances(attr_name: &str, value: &Value) -> Result<(), AndromedaError> {
    let instances = match value {
        Value::Array(arr) => arr.as_slice(),
        _ => return Ok(()),
    };

    for instance in instances {
        let Some(obj) = instance.as_object() else {
            continue;
        };

        let types: Vec<&str> = obj
            .get("@type")
            .and_then(|t| t.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        let is_property = types.iter().any(|t| *t == iri::PROPERTY.as_str());
        let is_geo_property = types.iter().any(|t| *t == iri::GEO_PROPERTY.as_str());
        let is_relationship = types.iter().any(|t| *t == iri::RELATIONSHIP.as_str());

        if (is_property || is_geo_property) && !obj.contains_key(iri::HAS_VALUE.as_str()) {
            let short_type = if is_geo_property { "GeoProperty" } else { "Property" };
            return Err(AndromedaError::bad_request_data(format!(
                "{} '{}' is missing required field 'value'",
                short_type, attr_name
            )));
        }

        if is_relationship && !obj.contains_key(iri::HAS_OBJECT.as_str()) {
            return Err(AndromedaError::bad_request_data(format!(
                "Relationship '{}' is missing required field 'object'",
                attr_name
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared sysAttrs injection helpers (used by query + notification_worker)
// ---------------------------------------------------------------------------

pub(crate) fn expanded_entity_object_mut(entity: &mut Value) -> Option<&mut Map<String, Value>> {
    match entity {
        Value::Array(arr) => arr.first_mut().and_then(|v| v.as_object_mut()),
        Value::Object(obj) => Some(obj),
        _ => None,
    }
}

pub(crate) fn inject_sys_attrs(entity: &mut Value, created_at: DateTime<Utc>, modified_at: DateTime<Utc>, expires_at: impl Into<Option<DateTime<Utc>>>) {
    let Some(obj) = expanded_entity_object_mut(entity) else {
        return;
    };

    let created_at_value = serde_json::json!([{
        "@type": "https://uri.etsi.org/ngsi-ld/DateTime",
        "@value": created_at.to_rfc3339_opts(SecondsFormat::Millis, true)
    }]);
    let modified_at_value = serde_json::json!([{
        "@type": "https://uri.etsi.org/ngsi-ld/DateTime",
        "@value": modified_at.to_rfc3339_opts(SecondsFormat::Millis, true)
    }]);

    obj.entry("https://uri.etsi.org/ngsi-ld/createdAt".to_string())
        .or_insert(created_at_value.clone());
    obj.entry("https://uri.etsi.org/ngsi-ld/modifiedAt".to_string())
        .or_insert(modified_at_value.clone());

    if let Some(expires_at) = expires_at.into() {
        let expires_at_value = serde_json::json!([{
            "@type": "https://uri.etsi.org/ngsi-ld/DateTime",
            "@value": expires_at.to_rfc3339_opts(SecondsFormat::Millis, true)
        }]);
        obj.entry("https://uri.etsi.org/ngsi-ld/expiresAt".to_string()).or_insert(expires_at_value);
    }

    inject_attr_sys_attrs(obj, &created_at_value, &modified_at_value);
}

pub(crate) fn inject_attr_sys_attrs(obj: &mut Map<String, Value>, created_at_value: &Value, modified_at_value: &Value) {
    let attr_keys: Vec<String> = obj
        .keys()
        .filter(|k| !k.starts_with('@') && (!k.starts_with("https://uri.etsi.org/ngsi-ld/") || k.starts_with("https://uri.etsi.org/ngsi-ld/default-context/")))
        .cloned()
        .collect();

    for key in attr_keys {
        let Some(arr) = obj.get_mut(&key).and_then(|v| v.as_array_mut()) else {
            continue;
        };
        for instance in arr.iter_mut() {
            let Some(inst_obj) = instance.as_object_mut() else {
                continue;
            };
            inst_obj
                .entry("https://uri.etsi.org/ngsi-ld/createdAt".to_string())
                .or_insert(created_at_value.clone());
            inst_obj
                .entry("https://uri.etsi.org/ngsi-ld/modifiedAt".to_string())
                .or_insert(modified_at_value.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_entity_attributes_wraps_concise_values_as_property() {
        let mut payload = serde_json::json!({
            "id": "urn:ngsi-ld:Testing:1",
            "type": "Testing",
            "A1": "value of A1",
            "A2": { "nested": true }
        });

        normalize_entity_attributes(&mut payload);

        assert_eq!(payload["A1"]["type"], "Property");
        assert_eq!(payload["A1"]["value"], "value of A1");
        assert_eq!(payload["A2"]["type"], "Property");
        assert_eq!(payload["A2"]["value"], serde_json::json!({ "nested": true }));
    }

    #[test]
    fn normalize_entity_attributes_keeps_typed_attributes() {
        let mut payload = serde_json::json!({
            "id": "urn:ngsi-ld:Testing:2",
            "type": "Testing",
            "name": {
                "type": "Property",
                "value": "Andromeda"
            }
        });

        normalize_entity_attributes(&mut payload);

        assert_eq!(payload["name"]["type"], "Property");
        assert_eq!(payload["name"]["value"], "Andromeda");
    }

    #[test]
    fn normalize_entity_attributes_wraps_concise_attr_entries() {
        let mut payload = serde_json::json!({
            "A1": 1,
            "A2": false
        });

        normalize_entity_attributes(&mut payload);

        assert_eq!(payload["A1"]["type"], "Property");
        assert_eq!(payload["A1"]["value"], 1);
        assert_eq!(payload["A2"]["type"], "Property");
        assert_eq!(payload["A2"]["value"], false);
    }
}
