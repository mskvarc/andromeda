//! Shared subscription cache for fast in-memory matching.
//!
//! Extracts the compilation and caching logic formerly owned by
//! `NotificationWorker` into a shared, concurrency-safe structure
//! backed by `DashMap`. Used by both the notification worker (event
//! matching) and the entity service (outbox entry generation).

use crate::{
    repository::subscription_store::repo::SubscriptionStore,
    service::{
        compiled_subscription::{CompiledGeoQuery, CompiledScopePattern, CompiledScopeTerm, CompiledSelector, CompiledSubscription},
        spatial::{
            SpatialKey,
            subscription_index::{SpatialSubscriptionIndex, SubEntry},
        },
        subscription_matching::find_matching,
    },
    tenancy::types::TenantRef,
    transport::endpoint::parse_endpoint_transport,
};
use andromeda_domain::{
    notification::{ChangeEvent, SubscriptionChange, TriggerReason},
    subscription::{EntitySelector, Subscription, SubscriptionStatus},
    sys_attrs::SysAttrsInclusion,
    vocab,
};
use andromeda_query::filter::parser;
use dashmap::DashMap;
use iri_rs::IriBuf;
use regex::Regex;
use std::{collections::HashSet, sync::Arc};
use tracing::debug;

/// Thread-safe, lock-free subscription cache for fast event matching.
///
/// Stores pre-compiled subscriptions keyed by subscription ID. Uses
/// `DashMap` for concurrent read/write access without explicit locking.
///
/// Maintains two auxiliary indexes so event matching scans only the
/// subscriptions that plausibly match the event type, instead of every
/// active subscription:
///
/// - `by_type`: expanded type IRI -> set of subscription ids whose selectors
///   include that type. A subscription with N selectors is indexed under all
///   N types.
/// - `untyped`: subscription ids with no selectors, or selectors whose types
///   are alternation regex patterns that cannot be resolved to a single IRI.
///   These are always candidates regardless of event type.
#[derive(Debug)]
pub struct SubscriptionCache {
    inner: DashMap<String, Arc<CompiledSubscription>>,
    by_type: DashMap<String, HashSet<String>>,
    untyped: dashmap::DashSet<String>,
    spatial: Arc<SpatialSubscriptionIndex>,
    tenant: TenantRef,
}

impl SubscriptionCache {
    /// Create an empty subscription cache scoped to the given tenant.
    pub fn new(tenant: TenantRef) -> SubscriptionCache {
        SubscriptionCache::with_spatial(tenant, Arc::new(SpatialSubscriptionIndex::new()))
    }

    /// Create an empty subscription cache that writes its geo index into
    /// the given shared [`SpatialSubscriptionIndex`].
    pub fn with_spatial(tenant: TenantRef, spatial: Arc<SpatialSubscriptionIndex>) -> SubscriptionCache {
        SubscriptionCache {
            inner: DashMap::new(),
            by_type: DashMap::new(),
            untyped: dashmap::DashSet::new(),
            spatial,
            tenant,
        }
    }

    /// Reload the entire cache from the subscription repository.
    ///
    /// Clears existing entries and replaces them with all currently active
    /// subscriptions. Failed compilations are logged and skipped.
    pub async fn reload(&self, repo: &impl SubscriptionStore) -> Result<(), crate::core::error::AndromedaError> {
        let subscriptions = repo.list_active().await?;
        self.inner.clear();
        self.by_type.clear();
        self.untyped.clear();
        for sub in subscriptions {
            if let Some(compiled) = compile_subscription(&sub) {
                self.index_insert(&compiled);
                self.inner.insert(sub.id.to_string(), Arc::new(compiled));
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
                        if sub.lifecycle.status == SubscriptionStatus::Active {
                            if let Some(compiled) = compile_subscription(&sub) {
                                self.index_remove(&id_str);
                                self.index_insert(&compiled);
                                self.inner.insert(id_str, Arc::new(compiled));
                            }
                        } else {
                            self.index_remove(&id_str);
                            self.inner.remove(&id_str);
                        }
                    }
                    Ok(None) => {
                        self.index_remove(&id_str);
                        self.inner.remove(&id_str);
                    }
                    Err(e) => {
                        debug!("Failed to load subscription {} for cache update: {}", id_str, e);
                    }
                }
            }
            SubscriptionChange::Deleted(id) => {
                let id_str = id.to_string();
                self.index_remove(&id_str);
                self.inner.remove(&id_str);
            }
        }
    }

    /// Find all compiled subscriptions that match the given change event.
    ///
    /// Gathers the candidate set from the type index (`by_type` entries for
    /// each of the event's types, union with `untyped`), then evaluates the
    /// full matching predicate only on those candidates. Uses lock-free
    /// `DashMap`/`DashSet` reads throughout.
    pub fn find_matching(&self, event: &ChangeEvent) -> Vec<Arc<CompiledSubscription>> {
        let mut candidate_ids: HashSet<String> = self.untyped.iter().map(|entry| entry.key().clone()).collect();
        for event_type in &event.entity_types {
            if let Some(entry) = self.by_type.get(event_type.as_str()) {
                for id in entry.value() {
                    candidate_ids.insert(id.clone());
                }
            }
        }

        let candidates = candidate_ids
            .into_iter()
            .filter_map(|id| self.inner.get(&id).map(|entry| Arc::clone(entry.value())));
        find_matching(candidates, event)
    }

    /// Look up a single compiled subscription by ID.
    pub fn get(&self, id: &str) -> Option<Arc<CompiledSubscription>> {
        self.inner.get(id).map(|entry| Arc::clone(entry.value()))
    }

    /// Tenant scope of this cache (used by callers building [`SpatialKey`]s
    /// against the shared geo index).
    pub const fn tenant(&self) -> &TenantRef {
        &self.tenant
    }

    /// Shared handle to the geo index this cache populates.
    pub fn spatial(&self) -> Arc<SpatialSubscriptionIndex> {
        Arc::clone(&self.spatial)
    }

    fn index_insert(&self, compiled: &CompiledSubscription) {
        self.spatial_insert(compiled);
        if compiled.selectors.is_empty() {
            self.untyped.insert(compiled.id.clone());
            return;
        }
        // A subscription with a selector whose type field is an alternation
        // (regex-backed) can match multiple expanded IRIs that the cache
        // cannot enumerate ahead of time; fall back to the untyped bucket so
        // `find_matching` still considers it.
        let mut indexed_any = false;
        for selector in &compiled.selectors {
            if selector.type_pattern.is_some() {
                self.untyped.insert(compiled.id.clone());
            } else {
                self.by_type.entry(selector.type_.clone()).or_default().insert(compiled.id.clone());
            }
            indexed_any = true;
        }
        if !indexed_any {
            self.untyped.insert(compiled.id.clone());
        }
    }

    fn index_remove(&self, id: &str) {
        self.spatial.remove(id);
        self.untyped.remove(id);
        // Drop the id from every type bucket it appears in. `retain` scans
        // the map once; the keys per bucket are short (one `HashSet<String>`
        // per indexed entity type) so removal stays cheap.
        self.by_type.iter_mut().for_each(|mut entry| {
            entry.value_mut().remove(id);
        });
        self.by_type.retain(|_, ids| !ids.is_empty());
    }

    /// Returns the number of cached subscriptions.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    fn spatial_insert(&self, compiled: &CompiledSubscription) {
        let Some(geo_q) = compiled.compiled_geo_q.as_ref() else {
            return;
        };
        let Ok(geoproperty_iri) = IriBuf::new(geo_q.geoproperty_iri.clone()) else {
            debug!(
                "Subscription {} spatial index skipped: invalid geoproperty IRI '{}'",
                compiled.id, geo_q.geoproperty_iri
            );
            return;
        };

        let Some(entry) = SubEntry::from_compiled(compiled.id.clone(), geo_q) else {
            debug!("Subscription {} spatial index skipped: geometry has no bounding rect", compiled.id);
            return;
        };

        // A subscription without selectors matches every type; index under a
        // synthetic wildcard key so find_matching can look it up regardless
        // of event type. Concretely-typed subscriptions index under every
        // selector type they name. Pattern-typed selectors fall back to the
        // wildcard bucket -- matches the `untyped` fallback used by the
        // by_type index.
        let mut indexed_any_type = false;
        for selector in &compiled.selectors {
            if selector.type_pattern.is_some() {
                continue;
            }
            match IriBuf::new(selector.type_.clone()) {
                Ok(type_iri) => {
                    let key = SpatialKey::new(self.tenant.clone(), type_iri, geoproperty_iri.clone());
                    self.spatial.insert(key, entry.clone());
                    indexed_any_type = true;
                }
                Err(_) => {
                    debug!(
                        "Subscription {} spatial index skipped selector type '{}': not a valid IRI",
                        compiled.id, selector.type_
                    );
                }
            }
        }
        if !indexed_any_type {
            // No concrete type selector -> index under a wildcard type IRI so
            // the cache can still reach the subscription when the caller
            // enumerates keys for cross-type matching.
            if let Ok(wildcard) = IriBuf::new("urn:andromeda:spatial:any-type".to_string()) {
                let key = SpatialKey::new(self.tenant.clone(), wildcard, geoproperty_iri);
                self.spatial.insert(key, entry);
            }
        }
    }
}

/// Compile a domain subscription into a pre-compiled form for fast matching.
///
/// Returns `None` if the subscription's endpoint transport cannot be parsed
/// (logged at debug level).
pub fn compile_subscription(sub: &Subscription) -> Option<CompiledSubscription> {
    let selectors: Vec<CompiledSelector> = sub
        .selection
        .entities
        .as_ref()
        .map(|entities| entities.iter().filter_map(compile_selector).collect())
        .unwrap_or_default();

    let triggers: Vec<String> = sub.selection.notification_trigger.as_ref().map_or_else(
        || TriggerReason::defaults().iter().map(|s| String::from(*s)).collect(),
        |t| t.iter().map(|nt| nt.to_string()).collect(),
    );

    let watched_attrs = sub
        .selection
        .watched_attributes
        .as_ref()
        .map(|attrs| attrs.iter().map(ToString::to_string).collect::<HashSet<String>>());

    let compiled_q = sub.filters.q.as_ref().and_then(|q_str| match parser::parse(q_str) {
        Ok(expr) => Some(expr),
        Err(e) => {
            debug!("Failed to parse q filter '{}' for subscription {}: {}", q_str, sub.id, e);
            None
        }
    });

    let compiled_csf = sub.filters.csf.as_ref().and_then(|csf_str| match parser::parse(csf_str) {
        Ok(expr) => Some(expr),
        Err(e) => {
            debug!("Failed to parse csf filter '{}' for subscription {}: {}", csf_str, sub.id, e);
            None
        }
    });

    let endpoint_transport = match parse_endpoint_transport(sub.notification.endpoint.uri.as_str(), sub.notification.endpoint.notifier_info.as_deref()) {
        Ok(t) => t,
        Err(e) => {
            debug!("Failed to parse endpoint transport for subscription {}: {}", sub.id, e);
            return None;
        }
    };

    Some(CompiledSubscription {
        id: sub.id.to_string(),
        status: sub.lifecycle.status,
        endpoint_uri: sub.notification.endpoint.uri.to_string(),
        endpoint_accept: sub.notification.endpoint.accept.as_ref().map(ToString::to_string),
        endpoint_receiver_info: sub.notification.endpoint.receiver_info.clone(),
        endpoint_transport,
        triggers,
        watched_attrs,
        selectors,
        throttling_secs: sub.delivery.throttling,
        time_interval_secs: sub.delivery.time_interval,
        notification_attributes: sub.notification.attributes.clone(),
        notification_format: sub.notification.format.map(|f| f.to_string()),
        notification_sys_attrs: if sub.notification.sys_attrs.unwrap_or(false) {
            SysAttrsInclusion::Include
        } else {
            SysAttrsInclusion::Strip
        },
        notification_show_changes: sub.notification.show_changes.unwrap_or(false),
        jsonld_context: sub.delivery.jsonld_context.as_ref().map(ToString::to_string),
        expires_at: sub.lifecycle.expires_at,
        compiled_q,
        compiled_geo_q: sub.filters.geo_q.as_ref().and_then(|v| compile_geo_q(v, sub.id.as_ref())),
        compiled_scope_q: sub.filters.scope_q.as_deref().map(compile_scope_q),
        dataset_id: sub.filters.dataset_id.as_ref().map(|ids| ids.iter().map(ToString::to_string).collect()),
        notification_join: sub.notification.join.map(|j| j.to_string()),
        notification_join_level: sub.notification.join_level.unwrap_or(1).min(5),
        compiled_csf,
        local_only: sub.delivery.local_only.unwrap_or(false),
    })
}

/// Pre-parse a `geoQ` JSON value into [`CompiledGeoQuery`].
///
/// Returns `None` when any required field is missing or malformed — logged at
/// debug level. The subscription then skips the geo path entirely (it never
/// matches events through geoQ) rather than re-parsing on every event.
pub fn compile_geo_q(geo_q: &serde_json::Value, sub_id: &str) -> Option<CompiledGeoQuery> {
    let geo_obj = geo_q.as_object()?;
    let georel_str = geo_obj.get("georel").and_then(|v| v.as_str())?;
    let geometry_str = geo_obj.get("geometry").and_then(|v| v.as_str())?;
    let coordinates_value = geo_obj.get("coordinates")?;
    let coordinates = match coordinates_value {
        serde_json::Value::String(s) => serde_json::from_str::<serde_json::Value>(s).ok()?,
        other => other.clone(),
    };
    let geoproperty_iri = geo_obj
        .get("geoproperty")
        .and_then(|v| v.as_str())
        .map_or_else(|| vocab::expanded::properties::LOCATION.as_str().to_owned(), ToOwned::to_owned);

    let geojson_value = serde_json::json!({
        "type": geometry_str,
        "coordinates": coordinates,
    });
    let geojson_str = geojson_value.to_string();
    let ref_geometry = match geojson_str.parse::<geojson::GeoJson>() {
        Ok(geojson::GeoJson::Geometry(g)) => match geo::Geometry::try_from(g) {
            Ok(geom) => geom,
            Err(e) => {
                debug!("Failed to convert geoQ geometry for subscription {}: {}", sub_id, e);
                return None;
            }
        },
        Ok(_) => {
            debug!("geoQ for subscription {} is not a geometry object", sub_id);
            return None;
        }
        Err(e) => {
            debug!("Failed to parse geoQ GeoJSON for subscription {}: {}", sub_id, e);
            return None;
        }
    };

    let georel = georel_str.parse().ok()?;

    Some(CompiledGeoQuery {
        ref_geometry,
        georel,
        geoproperty_iri,
    })
}

/// Pre-parse a `scopeQ` string into its disjunctive-normal-form term tree.
///
/// Terms that use `/+` are compiled to regex here so event matching can skip
/// regex construction on every event. Unrecognized patterns fall through to
/// exact-match semantics.
pub fn compile_scope_q(raw: &str) -> CompiledScopePattern {
    let or_terms = raw
        .split(',')
        .map(|or_term| or_term.split(';').map(|pattern| compile_scope_term(pattern.trim())).collect::<Vec<_>>())
        .collect();
    CompiledScopePattern { or_terms }
}

fn compile_scope_term(pattern: &str) -> CompiledScopeTerm {
    if let Some(prefix) = pattern.strip_suffix("/#") {
        return CompiledScopeTerm::HierarchicalWildcard { prefix: prefix.to_owned() };
    }
    if pattern.contains("/+") {
        let regex_pattern = format!("^{}$", pattern.replace("/+", "/[^/]+"));
        if let Ok(re) = Regex::new(&regex_pattern) {
            return CompiledScopeTerm::Regex(re);
        }
    }
    CompiledScopeTerm::Exact(pattern.to_owned())
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
    use super::*;
    use crate::transport::endpoint::EndpointTransport;
    use andromeda_domain::subscription::SubscriptionStatus;

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
            notification_sys_attrs: andromeda_domain::sys_attrs::SysAttrsInclusion::Strip,
            notification_show_changes: false,
            jsonld_context: None,
            expires_at: None,
            compiled_q: None,
            compiled_geo_q: None,
            compiled_scope_q: None,
            dataset_id: None,
            notification_join: None,
            notification_join_level: 1,
            compiled_csf: None,
            local_only: false,
        }
    }

    #[test]
    fn len_reflects_cache_size() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault);
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
