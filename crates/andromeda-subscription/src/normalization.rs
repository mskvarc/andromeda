//! Subscription persistence normalization.

use andromeda_jsonld::{
    compaction::{compact_attr_iri, compact_type_iri},
    context_spec::ContextSpec,
    engine::processor::JsonLdEngine,
};
use chrono::Utc;
use serde_json::Value;

pub fn fixup_integer_fields(value: &mut Value) {
    if let Some(obj) = value.as_object_mut() {
        for key in &["timeInterval", "throttling"] {
            if let Some(val) = obj.get(*key).and_then(|v| v.as_f64())
                && val.fract() == 0.0
            {
                obj.insert(key.to_string(), Value::Number(serde_json::Number::from(val as i64)));
            }
        }
    }
}

/// Inject default notificationTrigger if absent.
pub fn fixup_default_notification_trigger(value: &mut Value) {
    if value.get("notificationTrigger").is_none()
        && let Some(obj) = value.as_object_mut()
    {
        obj.insert("notificationTrigger".to_string(), serde_json::json!(["attributeCreated", "attributeUpdated"]));
    }
}

/// Parse stringified jsonldContext back to structured JSON.
pub fn fixup_jsonld_context(value: &mut Value) {
    let parsed = value.get("jsonldContext").and_then(|v| v.as_str()).and_then(|s| {
        let trimmed = s.trim();
        if trimmed.starts_with('[') || trimmed.starts_with('{') {
            serde_json::from_str::<Value>(trimmed).ok()
        } else {
            None
        }
    });
    if let Some(parsed) = parsed
        && let Some(obj) = value.as_object_mut()
    {
        obj.insert("jsonldContext".to_string(), parsed);
    }
}

/// Compute expired status at read time based on expiresAt.
pub fn fixup_expired_status(value: &mut Value, expires_at: Option<chrono::DateTime<Utc>>) {
    if let Some(expires_at) = expires_at
        && expires_at <= Utc::now()
        && let Some(obj) = value.as_object_mut()
    {
        obj.insert("status".to_string(), Value::String("expired".to_string()));
        obj.insert("isActive".to_string(), Value::Bool(false));
    }
}

/// Compact IRI-bearing fields in a serialized Subscription for HTTP response.
pub async fn compact_subscription_iris(engine: &JsonLdEngine, value: &mut Value, context: &ContextSpec) {
    // Compact entities[].type IRIs
    if let Some(entities) = value.get_mut("entities")
        && let Some(arr) = entities.as_array_mut()
    {
        for entity_sel in arr.iter_mut() {
            if let Some(type_iri) = entity_sel.get("type").and_then(|v| v.as_str()).map(String::from) {
                let compacted = compact_type_iri(engine, &type_iri, context).await;
                if let Some(obj) = entity_sel.as_object_mut() {
                    obj.insert("type".to_string(), Value::String(compacted));
                }
            }
        }
    }

    // Compact watchedAttributes IRIs
    if let Some(watched) = value.get_mut("watchedAttributes")
        && let Some(arr) = watched.as_array_mut()
    {
        for attr in arr.iter_mut() {
            if let Some(iri) = attr.as_str().map(String::from) {
                let compacted = compact_attr_iri(engine, &iri, context).await;
                *attr = Value::String(compacted);
            }
        }
    }

    // Compact notification.attributes IRIs
    if let Some(notification) = value.get_mut("notification")
        && let Some(attrs) = notification.get_mut("attributes")
        && let Some(arr) = attrs.as_array_mut()
    {
        for attr in arr.iter_mut() {
            if let Some(iri) = attr.as_str().map(String::from) {
                let compacted = compact_attr_iri(engine, &iri, context).await;
                *attr = Value::String(compacted);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::normalization::{fixup_default_notification_trigger, fixup_integer_fields};

    #[test]
    fn integral_delivery_intervals_serialize_as_json_integers() {
        let mut value = serde_json::json!({"timeInterval": 5.0, "throttling": 2.0});
        fixup_integer_fields(&mut value);
        assert_eq!(value, serde_json::json!({"timeInterval": 5, "throttling": 2}));
    }

    #[test]
    fn absent_notification_trigger_receives_the_clause_default() {
        let mut value = serde_json::json!({});
        fixup_default_notification_trigger(&mut value);
        assert_eq!(value["notificationTrigger"], serde_json::json!(["attributeCreated", "attributeUpdated"]));
    }
}

// ---------------------------------------------------------------------------
// Field extraction from payload (shared by create)
// ---------------------------------------------------------------------------
