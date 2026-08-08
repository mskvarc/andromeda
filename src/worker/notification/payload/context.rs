//! Notification context selection and compacted-entity normalization.

use crate::worker::notification::payload::{
    CompiledSubscription,
    ContextSpec,
    CoreContextPolicy,
    IriRefBuf,
    SysAttrsInclusion,
    Url,
    Value,
    expanded_properties,
};

pub fn context_for_subscription(sub: &CompiledSubscription) -> ContextSpec {
    context_for_subscription_raw(sub.jsonld_context.as_ref().map(Url::as_str))
}

/// Build a ContextSpec from an optional jsonldContext string.
/// Shared by both notification_worker and interval_worker.
pub fn context_for_subscription_raw(jsonld_context: Option<&str>) -> ContextSpec {
    let Some(url) = jsonld_context else {
        return ContextSpec::none().with_core_policy(CoreContextPolicy::Append);
    };
    let trimmed = url.trim();
    if ((trimmed.starts_with('{') && trimmed.ends_with('}')) || (trimmed.starts_with('[') && trimmed.ends_with(']')))
        && let Ok(value) = serde_json::from_str::<Value>(trimmed)
    {
        return ContextSpec::none().with_body_context(Some(value)).with_core_policy(CoreContextPolicy::Append);
    }
    // A jsonldContext that parses as an IRI reference is a context URL; an inline
    // object or array is a body context, handled above.
    if let Ok(iri) = IriRefBuf::new(url.to_string()) {
        return ContextSpec::from_links(vec![iri]).with_core_policy(CoreContextPolicy::Append);
    }
    ContextSpec::none().with_core_policy(CoreContextPolicy::Append)
}

pub fn normalize_compacted_entity(mut entity: Value) -> Value {
    if let Value::Object(obj) = &mut entity {
        if !obj.contains_key("id")
            && let Some(id) = obj.remove("@id")
        {
            obj.insert("id".to_string(), id);
        }
        if !obj.contains_key("type")
            && let Some(t) = obj.remove("@type")
        {
            let normalized = match t {
                Value::Array(arr) if arr.len() == 1 => arr.into_iter().next().unwrap_or(Value::Null),
                other => other,
            };
            obj.insert("type".to_string(), normalized);
        }
    }
    entity
}

pub fn strip_to_structural_keys(entity: &Value, sys_attrs: SysAttrsInclusion) -> Value {
    let Some(obj) = entity.as_object() else {
        return entity.clone();
    };
    let mut stripped = serde_json::Map::new();
    for (key, value) in obj {
        let keep = key == "@id"
            || key == "@type"
            || key == expanded_properties::DELETED_AT.as_str()
            || (sys_attrs == SysAttrsInclusion::Include
                && (key == expanded_properties::CREATED_AT.as_str() || key == expanded_properties::MODIFIED_AT.as_str()));
        if keep {
            stripped.insert(key.clone(), value.clone());
        }
    }
    Value::Object(stripped)
}
