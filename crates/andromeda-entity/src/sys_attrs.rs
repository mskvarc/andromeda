//! Injecting the clause 4.8 system attributes into an expanded entity.
//!
//! `createdAt` and `modifiedAt` live in columns rather than in the payload, so
//! any representation that asks for them (`options=sysAttrs`) has them put back
//! before rendering.

use andromeda_ngsi_ld_lexicon::rfc3339::whole_seconds;
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_ngsi_ld_vocabulary::{
    expanded::{classes as expanded_classes, properties as expanded_properties},
    prefix,
};
use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

/// A copy of an expanded entity with its clause-4.8 system attributes embedded.
///
/// A deletion's durable notification stores exactly this payload: once the row
/// is gone the delivery worker cannot re-read `createdAt`/`modifiedAt` from the
/// store the way a non-deletion notification does, so they are captured into the
/// snapshot at delete time. `deletedAt` is not embedded here — the delivery path
/// derives it from the event instant.
#[must_use]
pub fn snapshot_with_sys_attrs(entity: &Value, sys_attrs: &SysAttrsSnapshot) -> Value {
    let mut enriched = entity.clone(); // clone: the durable match owns a sys-attr-enriched copy of the pre-deletion payload
    inject_sys_attrs(&mut enriched, sys_attrs.created_at, sys_attrs.modified_at, sys_attrs.expires_at);
    enriched
}

/// Return a mutable reference to the top-level JSON object of an expanded entity.
pub fn expanded_entity_object_mut(entity: &mut Value) -> Option<&mut Map<String, Value>> {
    match entity {
        Value::Array(arr) => arr.first_mut().and_then(|v| v.as_object_mut()),
        Value::Object(obj) => Some(obj),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }
}

/// Inject system attributes (`createdAt`, `modifiedAt`, `expiresAt`) into an
/// expanded entity and each of its attribute instances.
pub fn inject_sys_attrs(entity: &mut Value, created_at: DateTime<Utc>, modified_at: DateTime<Utc>, expires_at: impl Into<Option<DateTime<Utc>>>) {
    let Some(obj) = expanded_entity_object_mut(entity) else {
        return;
    };
    let default_context_prefix = format!("{}default-context/", prefix::NGSI_LD.as_str());

    let created_at_value = serde_json::json!([{
        "@type": expanded_classes::DATE_TIME.as_str(),
        "@value": whole_seconds(created_at)
    }]);
    let modified_at_value = serde_json::json!([{
        "@type": expanded_classes::DATE_TIME.as_str(),
        "@value": whole_seconds(modified_at)
    }]);

    obj.entry(expanded_properties::CREATED_AT.as_str().to_string())
        .or_insert(created_at_value.clone()); // clone: value also passed to inject_attr_sys_attrs below
    obj.entry(expanded_properties::MODIFIED_AT.as_str().to_string())
        .or_insert(modified_at_value.clone()); // clone: value also passed to inject_attr_sys_attrs below

    if let Some(expires_at) = expires_at.into() {
        let expires_at_value = serde_json::json!([{
            "@type": expanded_classes::DATE_TIME.as_str(),
            "@value": whole_seconds(expires_at)
        }]);
        obj.entry(expanded_properties::EXPIRES_AT.as_str().to_string()).or_insert(expires_at_value);
    }

    inject_attr_sys_attrs_with_prefix(obj, &created_at_value, &modified_at_value, &default_context_prefix);
}

/// Inject system attributes into each attribute instance within an entity object.
pub fn inject_attr_sys_attrs(obj: &mut Map<String, Value>, created_at_value: &Value, modified_at_value: &Value) {
    let default_context_prefix = format!("{}default-context/", prefix::NGSI_LD.as_str());
    inject_attr_sys_attrs_with_prefix(obj, created_at_value, modified_at_value, &default_context_prefix);
}

fn inject_attr_sys_attrs_with_prefix(obj: &mut Map<String, Value>, created_at_value: &Value, modified_at_value: &Value, default_context_prefix: &str) {
    let attr_keys: Vec<String> = obj
        .keys()
        .filter(|k| !k.starts_with('@') && (!k.starts_with(prefix::NGSI_LD.as_str()) || k.starts_with(default_context_prefix)))
        .cloned() // clone: owned keys needed to avoid borrowing obj during mutation below
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
                .entry(expanded_properties::CREATED_AT.as_str().to_string())
                .or_insert(created_at_value.clone()); // clone: or_insert requires owned value, reused across loop iterations
            inst_obj
                .entry(expanded_properties::MODIFIED_AT.as_str().to_string())
                .or_insert(modified_at_value.clone()); // clone: or_insert requires owned value, reused across loop iterations
        }
    }
}
