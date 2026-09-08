//! Deterministic merging of context-source discovery responses.

use crate::outcome::ForwardedDiscoveryResult;
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use indexmap::IndexMap;
use serde_json::Value;
use std::collections::HashSet;

/// Collect protocol warnings from all forwarded discovery outcomes in encounter order.
#[must_use]
pub fn collect_discovery_warnings(results: &[ForwardedDiscoveryResult]) -> Vec<NgsiLdWarning> {
    results.iter().flat_map(|r| r.warnings.iter().cloned()).collect()
}

/// Merge local types result with forwarded discovery results.
///
/// For simple types list (`typeList`): union of type IRIs, deduplicated.
/// For detailed types (`details=true`): merge by type name, union attribute names per type.
/// For type info (single type): merge attribute details from all sources.
#[must_use]
pub fn merge_types_results(local: Value, remote: Vec<ForwardedDiscoveryResult>, details: bool) -> Value {
    let bodies: Vec<&Value> = remote.iter().filter_map(|r| r.body.as_ref()).collect();
    if bodies.is_empty() {
        return local;
    }

    if details {
        merge_detailed_types(local, &bodies)
    } else {
        merge_simple_types(local, &bodies)
    }
}

/// Merge simple type lists by unioning `typeList` arrays and deduplicating.
fn merge_simple_types(mut local: Value, remote_bodies: &[&Value]) -> Value {
    let local_list = local.get_mut("typeList").and_then(|v| v.as_array_mut());
    let Some(local_list) = local_list else {
        return local;
    };

    let mut seen: HashSet<EntityTypeName> = local_list.iter().filter_map(Value::as_str).filter_map(|raw| raw.parse().ok()).collect();

    for body in remote_bodies {
        if let Some(arr) = body.get("typeList").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(entity_type) = item.as_str().and_then(|raw| raw.parse().ok())
                    && seen.insert(entity_type)
                {
                    local_list.push(item.clone()); // clone: adding remote type IRI to merged list
                }
            }
        }
        // Also handle bare arrays (some brokers may return array directly)
        if let Some(arr) = body.as_array() {
            for item in arr {
                if let Some(entity_type) = item.as_str().and_then(|raw| raw.parse().ok())
                    && seen.insert(entity_type)
                {
                    local_list.push(item.clone()); // clone: adding remote type IRI from bare array
                }
            }
        }
    }

    local
}

/// Merge detailed type lists by type name, unioning attribute names per type.
fn merge_detailed_types(local: Value, remote_bodies: &[&Value]) -> Value {
    let mut type_map: IndexMap<EntityTypeName, Value> = IndexMap::new();

    // Collect local types
    if let Some(arr) = local.as_array() {
        for entry in arr {
            if let Some(id) = entry.get("id").and_then(Value::as_str).and_then(|raw| raw.parse().ok()) {
                type_map.insert(id, entry.clone()); // clone: local type entry for merge map
            }
        }
    }

    // Merge remote types
    for body in remote_bodies {
        let entries = if let Some(arr) = body.as_array() {
            arr.as_slice()
        } else {
            continue;
        };
        for entry in entries {
            let Some(id) = entry.get("id").and_then(Value::as_str).and_then(|raw| raw.parse::<EntityTypeName>().ok()) else {
                continue;
            };
            if let Some(existing) = type_map.get_mut(&id) {
                merge_type_attribute_names(existing, entry);
            } else {
                type_map.insert(id, entry.clone()); // clone: remote type entry not present locally
            }
        }
    }

    Value::Array(type_map.into_values().collect())
}

/// Merge attribute names within a type detail entry (union of attributeNames arrays).
fn merge_type_attribute_names(local: &mut Value, remote: &Value) {
    if let Some(local_attrs) = local.get_mut("attributeNames").and_then(|v| v.as_array_mut())
        && let Some(remote_attrs) = remote.get("attributeNames").and_then(|v| v.as_array())
    {
        let mut existing: HashSet<AttributeName> = local_attrs.iter().filter_map(Value::as_str).filter_map(|raw| raw.parse().ok()).collect();
        for attr in remote_attrs {
            if let Some(name) = attr.as_str().and_then(|raw| raw.parse().ok())
                && existing.insert(name)
            {
                local_attrs.push(attr.clone()); // clone: adding remote attribute name to merged set
            }
        }
    }
}

/// Merge type info result (single type) with forwarded discovery results.
///
/// Merges attribute details from all sources, unioning attribute names.
#[must_use]
pub fn merge_type_info_results(local: Value, remote: Vec<ForwardedDiscoveryResult>) -> Value {
    let bodies: Vec<&Value> = remote.iter().filter_map(|r| r.body.as_ref()).collect();
    if bodies.is_empty() {
        return local;
    }

    let mut result = local;
    for body in &bodies {
        merge_type_attribute_names(&mut result, body);
    }
    result
}

/// Merge local attributes result with forwarded discovery results.
///
/// For simple attribute list (`attributeList`): union of attribute IRIs, deduplicated.
/// For detailed attributes (`details=true`): merge by attribute name, union type names per attribute.
#[must_use]
pub fn merge_attributes_results(local: Value, remote: Vec<ForwardedDiscoveryResult>, details: bool) -> Value {
    let bodies: Vec<&Value> = remote.iter().filter_map(|r| r.body.as_ref()).collect();
    if bodies.is_empty() {
        return local;
    }

    if details {
        merge_detailed_attributes(local, &bodies)
    } else {
        merge_simple_attributes(local, &bodies)
    }
}

/// Merge simple attribute lists by unioning `attributeList` arrays and deduplicating.
fn merge_simple_attributes(mut local: Value, remote_bodies: &[&Value]) -> Value {
    let local_list = local.get_mut("attributeList").and_then(|v| v.as_array_mut());
    let Some(local_list) = local_list else {
        return local;
    };

    let mut seen: HashSet<AttributeName> = local_list.iter().filter_map(Value::as_str).filter_map(|raw| raw.parse().ok()).collect();

    for body in remote_bodies {
        if let Some(arr) = body.get("attributeList").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(name) = item.as_str().and_then(|raw| raw.parse().ok())
                    && seen.insert(name)
                {
                    local_list.push(item.clone()); // clone: adding remote attribute IRI to merged list
                }
            }
        }
        if let Some(arr) = body.as_array() {
            for item in arr {
                if let Some(name) = item.as_str().and_then(|raw| raw.parse().ok())
                    && seen.insert(name)
                {
                    local_list.push(item.clone()); // clone: adding remote attribute IRI from bare array
                }
            }
        }
    }

    local
}

/// Merge detailed attribute lists by attribute name, unioning type names per attribute.
fn merge_detailed_attributes(local: Value, remote_bodies: &[&Value]) -> Value {
    let mut attr_map: IndexMap<AttributeName, Value> = IndexMap::new();

    if let Some(arr) = local.as_array() {
        for entry in arr {
            if let Some(id) = entry.get("id").and_then(Value::as_str).and_then(|raw| raw.parse().ok()) {
                attr_map.insert(id, entry.clone()); // clone: local attribute entry for merge map
            }
        }
    }

    for body in remote_bodies {
        let entries = if let Some(arr) = body.as_array() {
            arr.as_slice()
        } else {
            continue;
        };
        for entry in entries {
            let Some(id) = entry.get("id").and_then(Value::as_str).and_then(|raw| raw.parse::<AttributeName>().ok()) else {
                continue;
            };
            if let Some(existing) = attr_map.get_mut(&id) {
                merge_attribute_type_names(existing, entry);
            } else {
                attr_map.insert(id, entry.clone()); // clone: remote attribute entry not present locally
            }
        }
    }

    Value::Array(attr_map.into_values().collect())
}

/// Merge type names within an attribute detail entry (union of typeNames arrays).
fn merge_attribute_type_names(local: &mut Value, remote: &Value) {
    if let Some(local_types) = local.get_mut("typeNames").and_then(|v| v.as_array_mut())
        && let Some(remote_types) = remote.get("typeNames").and_then(|v| v.as_array())
    {
        let mut existing: HashSet<EntityTypeName> = local_types.iter().filter_map(Value::as_str).filter_map(|raw| raw.parse().ok()).collect();
        for t in remote_types {
            if let Some(entity_type) = t.as_str().and_then(|raw| raw.parse().ok())
                && existing.insert(entity_type)
            {
                local_types.push(t.clone()); // clone: adding remote type name to merged set
            }
        }
    }
}

/// Merge attribute info result (single attribute) with forwarded discovery results.
///
/// Merges type name lists from all sources.
#[must_use]
pub fn merge_attribute_info_results(local: Value, remote: Vec<ForwardedDiscoveryResult>) -> Value {
    let bodies: Vec<&Value> = remote.iter().filter_map(|r| r.body.as_ref()).collect();
    if bodies.is_empty() {
        return local;
    }

    let mut result = local;
    for body in &bodies {
        merge_attribute_type_names(&mut result, body);
    }
    result
}
