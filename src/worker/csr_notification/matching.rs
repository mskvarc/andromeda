//! Projection of context-source registration information for notification delivery.

use crate::worker::csr_notification::{AttributeName, CompiledCsrSubscription, Value};

/// Filter CSR information array to only include RegistrationInfo elements
/// that match the subscription's selectors and watched attributes.
pub(super) fn filter_csr_for_subscription(csr_data: &Value, sub: &CompiledCsrSubscription) -> Value {
    let mut filtered = csr_data.clone();

    if sub.selectors.is_empty() && sub.watched_attrs.is_none() {
        return filtered;
    }

    if let Some(information) = filtered.get_mut("information")
        && let Some(info_arr) = information.as_array()
    {
        let filtered_info: Vec<Value> = info_arr
            .iter()
            .filter(|info| {
                // Check entity selector match
                if !sub.selectors.is_empty() {
                    let entities = info.get("entities").and_then(|v| v.as_array());
                    let matches = match entities {
                        Some(ents) => sub.selectors.iter().any(|sel| {
                            ents.iter().any(|ent| {
                                let ent_type = ent.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                sel.type_.matches(&[ent_type])
                            })
                        }),
                        None => false,
                    };
                    if !matches {
                        return false;
                    }
                }

                // Check watched attributes match
                if let Some(ref watched) = sub.watched_attrs {
                    let prop_names: Vec<AttributeName> = info
                        .get("propertyNames")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter().filter_map(Value::as_str).filter_map(|raw| raw.parse().ok()).collect())
                        .unwrap_or_default();
                    let rel_names: Vec<AttributeName> = info
                        .get("relationshipNames")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter().filter_map(Value::as_str).filter_map(|raw| raw.parse().ok()).collect())
                        .unwrap_or_default();

                    if !prop_names.iter().any(|name| watched.contains(name)) && !rel_names.iter().any(|name| watched.contains(name)) {
                        return false;
                    }
                }

                true
            })
            .cloned()
            .collect();

        if let Some(obj) = filtered.as_object_mut() {
            obj.insert("information".to_string(), Value::Array(filtered_info));
        }
    }

    filtered
}
