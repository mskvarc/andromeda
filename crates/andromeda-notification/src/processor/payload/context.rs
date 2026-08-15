//! Notification context selection and compacted-entity normalization.

use andromeda_jsonld::context_spec::{ContextSpec, CoreContextPolicy};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use andromeda_subscription::compiled::CompiledSubscription;
use iri_rs::IriRefBuf;
use serde_json::Value;
use url::Url;

pub fn context_for_subscription(sub: &CompiledSubscription) -> ContextSpec {
    context_from_url(sub.jsonld_context.as_ref())
}

/// Build a context specification from an optional `jsonldContext` URL.
pub fn context_from_url(jsonld_context: Option<&Url>) -> ContextSpec {
    let Some(url) = jsonld_context else {
        return ContextSpec::none().with_core_policy(CoreContextPolicy::Append);
    };
    if let Ok(iri) = IriRefBuf::new(url.as_str().to_owned()) {
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
                Value::Array(arr) if arr.len() == 1 => match arr.into_iter().next() {
                    Some(value) => value,
                    None => Value::Null,
                },
                other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) | Value::Object(_)) => other,
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
