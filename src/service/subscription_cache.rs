//! Shared subscription cache for fast in-memory matching.
//!
//! Extracts the compilation and caching logic formerly owned by
//! `NotificationWorker` into a shared, concurrency-safe structure
//! backed by `DashMap`. Used by both the notification worker (event
//! matching) and the entity service (outbox entry generation).

use crate::{
    domain::{
        notification::{ChangeEvent, CompiledSelector, CompiledSubscription, SubscriptionChange, TriggerReason},
        subscription::{EntitySelector, Subscription, SubscriptionStatus},
    },
    query::q,
    renderer::SysAttrsInclusion,
    repository::subscription_store::SubscriptionStore,
    service::subscription_matching::find_matching,
    transport::endpoint::parse_endpoint_transport,
};
use dashmap::DashMap;
use regex::Regex;
use std::{collections::HashSet, sync::Arc};
use tracing::debug;

/// Thread-safe, lock-free subscription cache for fast event matching.
///
/// Stores pre-compiled subscriptions keyed by subscription ID. Uses
/// `DashMap` for concurrent read/write access without explicit locking.
#[derive(Debug)]
pub struct SubscriptionCache {
    inner: DashMap<String, Arc<CompiledSubscription>>,
}

impl SubscriptionCache {
    /// Create an empty subscription cache.
    pub fn new() -> SubscriptionCache {
        SubscriptionCache { inner: DashMap::new() }
    }

    /// Reload the entire cache from the subscription repository.
    ///
    /// Clears existing entries and replaces them with all currently active
    /// subscriptions. Failed compilations are logged and skipped.
    pub async fn reload(&self, repo: &impl SubscriptionStore) -> Result<(), crate::core::error::AndromedaError> {
        let subscriptions = repo.list_active().await?;
        self.inner.clear();
        for sub in subscriptions {
            if let Some(compiled) = compile_subscription(&sub) {
                self.inner.insert(sub.id.clone(), Arc::new(compiled)); // clone: owned key for DashMap, sub.id also consumed by compiled
            }
        }
        Ok(())
    }

    /// Handle a single subscription change event (upsert or removal).
    pub async fn handle_change(&self, change: &SubscriptionChange, repo: &impl SubscriptionStore) {
        match change {
            SubscriptionChange::Upserted(id) => {
                let id_str = id.to_string();
                match repo.get(&id_str).await {
                    Ok(Some(sub)) => {
                        if sub.status == SubscriptionStatus::Active {
                            if let Some(compiled) = compile_subscription(&sub) {
                                self.inner.insert(id_str, Arc::new(compiled));
                            }
                        } else {
                            self.inner.remove(&id_str);
                        }
                    }
                    Ok(None) => {
                        self.inner.remove(&id_str);
                    }
                    Err(e) => {
                        debug!("Failed to load subscription {} for cache update: {}", id_str, e);
                    }
                }
            }
            SubscriptionChange::Deleted(id) => {
                self.inner.remove(&id.to_string());
            }
        }
    }

    /// Find all compiled subscriptions that match the given change event.
    ///
    /// Iterates the `DashMap` without holding a global lock. Returns cloned
    /// `Arc` handles for lock-free downstream use.
    pub fn find_matching(&self, event: &ChangeEvent) -> Vec<Arc<CompiledSubscription>> {
        find_matching(self.inner.iter().map(|entry| Arc::clone(entry.value())), event)
    }

    /// Look up a single compiled subscription by ID.
    pub fn get(&self, id: &str) -> Option<Arc<CompiledSubscription>> {
        self.inner.get(id).map(|entry| Arc::clone(entry.value()))
    }

    /// Returns the number of cached subscriptions.
    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

/// Compile a domain subscription into a pre-compiled form for fast matching.
///
/// Returns `None` if the subscription's endpoint transport cannot be parsed
/// (logged at debug level).
pub fn compile_subscription(sub: &Subscription) -> Option<CompiledSubscription> {
    let selectors: Vec<CompiledSelector> = sub
        .entities
        .as_ref()
        .map(|entities| entities.iter().filter_map(compile_selector).collect())
        .unwrap_or_default();

    let triggers: Vec<String> = sub.notification_trigger.as_ref().map_or_else(
        || TriggerReason::defaults().iter().map(|s| String::from(*s)).collect(),
        |t| t.iter().map(|nt| nt.to_string()).collect(),
    );

    let watched_attrs = sub.watched_attributes.as_ref().map(|attrs| attrs.iter().cloned().collect::<HashSet<String>>()); // clone: owned strings for HashSet

    let compiled_q = sub.q.as_ref().and_then(|q_str| match q::parser::parse(q_str) {
        Ok(expr) => Some(expr),
        Err(e) => {
            debug!("Failed to parse q filter '{}' for subscription {}: {}", q_str, sub.id, e);
            None
        }
    });

    let compiled_csf = sub.csf.as_ref().and_then(|csf_str| match q::parser::parse(csf_str) {
        Ok(expr) => Some(expr),
        Err(e) => {
            debug!("Failed to parse csf filter '{}' for subscription {}: {}", csf_str, sub.id, e);
            None
        }
    });

    let endpoint_transport = match parse_endpoint_transport(&sub.notification.endpoint.uri, sub.notification.endpoint.notifier_info.as_deref()) {
        Ok(t) => t,
        Err(e) => {
            debug!("Failed to parse endpoint transport for subscription {}: {}", sub.id, e);
            return None;
        }
    };

    Some(CompiledSubscription {
        id: sub.id.clone(),
        status: sub.status,
        endpoint_uri: sub.notification.endpoint.uri.clone(),
        endpoint_accept: sub.notification.endpoint.accept.clone(),
        endpoint_receiver_info: sub.notification.endpoint.receiver_info.clone(),
        endpoint_transport,
        triggers,
        watched_attrs,
        selectors,
        throttling_secs: sub.throttling,
        time_interval_secs: sub.time_interval,
        notification_attributes: sub.notification.attributes.clone(),
        notification_format: sub.notification.format.map(|f| f.to_string()),
        notification_sys_attrs: if sub.notification.sys_attrs.unwrap_or(false) {
            SysAttrsInclusion::Include
        } else {
            SysAttrsInclusion::Strip
        },
        notification_show_changes: sub.notification.show_changes.unwrap_or(false),
        jsonld_context: sub.jsonld_context.clone(),
        expires_at: sub.expires_at,
        compiled_q,
        geo_q: sub.geo_q.clone(),
        scope_q: sub.scope_q.clone(),
        dataset_id: sub.dataset_id.clone(),
        notification_join: sub.notification.join.map(|j| j.to_string()),
        notification_join_level: sub.notification.join_level.unwrap_or(1).min(5),
        compiled_csf,
        local_only: sub.local_only.unwrap_or(false),
    })
}

/// Compile a single entity selector into a pre-compiled form.
fn compile_selector(selector: &EntitySelector) -> Option<CompiledSelector> {
    let id_pattern = selector.id_pattern.as_ref().and_then(|p| match Regex::new(p) {
        Ok(r) => Some(r),
        Err(e) => {
            debug!("Invalid idPattern regex '{}': {}", p, e);
            None
        }
    });

    let type_pattern = if selector.type_.contains('|') || (selector.type_.contains('(') && selector.type_.contains(')')) {
        let raw = selector.type_.trim();
        let inner = raw.trim_start_matches('(').trim_end_matches(')');
        let alternatives: Vec<&str> = inner.split('|').collect();
        let pattern_parts: Vec<String> = alternatives.iter().map(|alt| regex::escape(alt.trim())).collect();
        let regex_str = format!("({})", pattern_parts.join("|"));
        match Regex::new(&regex_str) {
            Ok(r) => Some(r),
            Err(e) => {
                debug!("Invalid type regex pattern '{}': {}", selector.type_, e);
                None
            }
        }
    } else {
        None
    };

    Some(CompiledSelector {
        id: selector.id.clone(),
        id_pattern,
        type_: selector.type_.clone(),
        type_pattern,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use crate::{
        domain::{
            notification::{ChangeOrigin, CompiledSelector, TriggerReason},
            subscription::SubscriptionStatus,
        },
        transport::endpoint::EndpointTransport,
    };

    fn make_compiled_sub(id: &str, type_: &str, triggers: Vec<&str>) -> CompiledSubscription {
        CompiledSubscription {
            id: id.to_string(),
            status: SubscriptionStatus::Active,
            endpoint_uri: "http://example.com/notify".to_string(),
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_transport: EndpointTransport::Http,
            triggers: triggers.into_iter().map(String::from).collect(),
            watched_attrs: None,
            selectors: vec![CompiledSelector {
                id: None,
                id_pattern: None,
                type_: type_.to_string(),
                type_pattern: None,
            }],
            throttling_secs: None,
            time_interval_secs: None,
            notification_attributes: None,
            notification_format: None,
            notification_sys_attrs: crate::renderer::SysAttrsInclusion::Strip,
            notification_show_changes: false,
            jsonld_context: None,
            expires_at: None,
            compiled_q: None,
            geo_q: None,
            scope_q: None,
            dataset_id: None,
            notification_join: None,
            notification_join_level: 1,
            compiled_csf: None,
            local_only: false,
        }
    }

    fn make_event(entity_id: &str, types: Vec<&str>, trigger: TriggerReason) -> ChangeEvent {
        ChangeEvent {
            entity_id: entity_id.parse().expect("valid test URI"),
            entity_types: types.into_iter().map(String::from).collect(),
            trigger_reason: trigger,
            changed_attributes: vec!["speed".to_string()],
            occurred_at: chrono::Utc::now(),
            entity_snapshot: None,
            previous_attributes: None,
            sys_attrs_snapshot: None,
            origin: ChangeOrigin::Local,
        }
    }

    #[test]
    fn find_matching_returns_correct_subscriptions_for_event() {
        let cache = SubscriptionCache::new();
        cache
            .inner
            .insert("sub1".to_string(), Arc::new(make_compiled_sub("sub1", "Vehicle", vec!["entityCreated"])));
        cache
            .inner
            .insert("sub2".to_string(), Arc::new(make_compiled_sub("sub2", "Building", vec!["entityCreated"])));
        cache
            .inner
            .insert("sub3".to_string(), Arc::new(make_compiled_sub("sub3", "Vehicle", vec!["entityUpdated"])));

        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], TriggerReason::EntityCreated);
        let matched = cache.find_matching(&event);

        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].id, "sub1");
    }

    #[test]
    fn find_matching_returns_empty_when_no_subscriptions_match() {
        let cache = SubscriptionCache::new();
        cache
            .inner
            .insert("sub1".to_string(), Arc::new(make_compiled_sub("sub1", "Building", vec!["entityCreated"])));

        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], TriggerReason::EntityCreated);
        let matched = cache.find_matching(&event);

        assert!(matched.is_empty());
    }

    #[test]
    fn find_matching_returns_multiple_matching_subscriptions() {
        let cache = SubscriptionCache::new();
        cache
            .inner
            .insert("sub1".to_string(), Arc::new(make_compiled_sub("sub1", "Vehicle", vec!["entityCreated"])));
        cache
            .inner
            .insert("sub2".to_string(), Arc::new(make_compiled_sub("sub2", "Vehicle", vec!["entityCreated"])));

        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], TriggerReason::EntityCreated);
        let matched = cache.find_matching(&event);

        assert_eq!(matched.len(), 2);
    }

    #[test]
    fn find_matching_skips_paused_subscriptions() {
        let cache = SubscriptionCache::new();
        let mut sub = make_compiled_sub("sub1", "Vehicle", vec!["entityCreated"]);
        sub.status = SubscriptionStatus::Paused;
        cache.inner.insert("sub1".to_string(), Arc::new(sub));

        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], TriggerReason::EntityCreated);
        let matched = cache.find_matching(&event);

        assert!(matched.is_empty());
    }

    #[test]
    fn find_matching_skips_expired_subscriptions() {
        let cache = SubscriptionCache::new();
        let mut sub = make_compiled_sub("sub1", "Vehicle", vec!["entityCreated"]);
        sub.expires_at = Some(chrono::Utc::now() - chrono::Duration::hours(1));
        cache.inner.insert("sub1".to_string(), Arc::new(sub));

        let event = make_event("urn:ngsi-ld:Vehicle:A1", vec!["Vehicle"], TriggerReason::EntityCreated);
        let matched = cache.find_matching(&event);

        assert!(matched.is_empty());
    }

    #[test]
    fn len_reflects_cache_size() {
        let cache = SubscriptionCache::new();
        assert_eq!(cache.len(), 0);

        cache
            .inner
            .insert("sub1".to_string(), Arc::new(make_compiled_sub("sub1", "Vehicle", vec!["entityCreated"])));
        assert_eq!(cache.len(), 1);

        cache
            .inner
            .insert("sub2".to_string(), Arc::new(make_compiled_sub("sub2", "Building", vec!["entityCreated"])));
        assert_eq!(cache.len(), 2);
    }
}
