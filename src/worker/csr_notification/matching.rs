//! Compilation, matching, and projection of context-source subscriptions.

use crate::worker::csr_notification::{
    Arc,
    AttributeName,
    CompiledCsrSubscription,
    DashMap,
    EntitySelector,
    HashSet,
    Subscription,
    SubscriptionStatus,
    Uri,
    Utc,
    Value,
    debug,
    eval,
    parse_endpoint_transport,
};

pub(super) fn compile_csr_subscription(sub: &Subscription) -> Option<CompiledCsrSubscription> {
    // clone: the compiled form outlives the borrowed subscription and every
    // selector member is already parsed
    let selectors: Vec<EntitySelector> = sub.selection.entities.clone().unwrap_or_default();

    let watched_attrs = sub.selection.watched_attributes.as_ref().cloned().map(HashSet::from_iter);

    let endpoint_transport = match parse_endpoint_transport(&sub.notification.endpoint.uri, sub.notification.endpoint.notifier_info.as_deref()) {
        Ok(t) => t,
        Err(e) => {
            debug!("Failed to parse endpoint transport for CSR subscription {}: {}", sub.id, e);
            return None;
        }
    };

    Some(CompiledCsrSubscription {
        // clone: the compiled form outlives the borrowed subscription
        id: sub.id.clone(),
        status: sub.lifecycle.status,
        endpoint_uri: sub.notification.endpoint.uri.clone(),
        // clone: the compiled form outlives the subscription it was built from
        endpoint_accept: sub.notification.endpoint.accept.clone(),
        endpoint_receiver_info: sub.notification.endpoint.receiver_info.clone(),
        endpoint_transport,
        selectors,
        watched_attrs,
        throttling_secs: sub.delivery.throttling,
        time_interval_secs: sub.delivery.time_interval,
        jsonld_context: sub.delivery.jsonld_context.clone(),
        expires_at: sub.lifecycle.expires_at,
        // clone: the compiled form owns its filter
        compiled_csf: sub.filters.csf.clone(),
    })
}

pub(super) fn find_matching_csr_subscriptions(cache: &DashMap<Uri, Arc<CompiledCsrSubscription>>, csr_data: &Value) -> Vec<Arc<CompiledCsrSubscription>> {
    cache
        .iter()
        .filter_map(|entry| {
            if matches_csr_subscription(entry.value(), csr_data) {
                Some(Arc::clone(entry.value()))
            } else {
                None
            }
        })
        .collect()
}

/// Match a CSR subscription against a CSR's data.
/// Matching logic (clause 5.12):
/// - Match subscription entity selectors against CSR information[].entities[]
/// - Match watched attrs against CSR information[].propertyNames + relationshipNames
pub(super) fn matches_csr_subscription(sub: &CompiledCsrSubscription, csr_data: &Value) -> bool {
    if sub.status != SubscriptionStatus::Active {
        return false;
    }

    if let Some(expires_at) = sub.expires_at
        && expires_at <= Utc::now()
    {
        return false;
    }

    // Skip time-interval subscriptions
    if sub.time_interval_secs.is_some() {
        return false;
    }

    // Entity selector matching: subscription selectors match against CSR information[].entities[]
    if !sub.selectors.is_empty() {
        let information = csr_data.get("information").and_then(|v| v.as_array());
        let any_selector_matches = sub.selectors.iter().any(|sel| match information {
            Some(info_arr) => info_arr.iter().any(|info| {
                let entities = info.get("entities").and_then(|v| v.as_array());
                match entities {
                    Some(ents) => ents.iter().any(|ent| {
                        let ent_type = ent.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        if !sel.type_.matches(&[ent_type]) {
                            return false;
                        }
                        if let Some(ref sel_id) = sel.id {
                            let ent_id = ent.get("id").and_then(|v| v.as_str()).unwrap_or("");
                            // `AsRef` hands back the identifier's own text; rendering it
                            // would allocate once per candidate in a matching loop.
                            if ent_id != sel_id.as_ref() {
                                return false;
                            }
                        }
                        if let Some(ref sel_pattern) = sel.id_pattern {
                            let ent_id = ent.get("id").and_then(|v| v.as_str()).unwrap_or("");
                            if !sel_pattern.is_match(ent_id) {
                                return false;
                            }
                        }
                        true
                    }),
                    None => false,
                }
            }),
            None => false,
        });

        if !any_selector_matches {
            return false;
        }
    }

    // Watched attributes: match against CSR propertyNames + relationshipNames
    if let Some(ref watched) = sub.watched_attrs {
        let information = csr_data.get("information").and_then(|v| v.as_array());
        let any_attr_matches = match information {
            Some(info_arr) => info_arr.iter().any(|info| {
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

                prop_names.iter().any(|name| watched.contains(name)) || rel_names.iter().any(|name| watched.contains(name))
            }),
            None => false,
        };

        if !any_attr_matches {
            return false;
        }
    }

    // Evaluate csf (Context Source Filter) against the CSR data
    if let Some(ref csf_expr) = sub.compiled_csf
        && !eval::evaluate(csf_expr, csr_data)
    {
        return false;
    }

    true
}

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
