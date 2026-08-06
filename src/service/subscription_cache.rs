//! Shared subscription cache for fast in-memory matching.
//!
//! Extracts the compilation and caching logic formerly owned by
//! `NotificationWorker` into a shared, concurrency-safe structure
//! backed by `DashMap`. Used by both the notification worker (event
//! matching) and the entity service (outbox entry generation).

use crate::service::{compiled_subscription::CompiledSubscription, subscription::store::SubscriptionStore, subscription_matching::find_matching};
use andromeda_delivery::endpoint::transport::parse_endpoint_transport;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    notification::{ChangeEvent, SubscriptionChange, TriggerReason},
    subscription::{EntitySelector, Subscription, SubscriptionStatus},
};
use andromeda_ngsi_ld_query::geo::query::GeoQuery;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use andromeda_spatial_index::{
    key::SpatialKey,
    subscription::{entry::SubEntry, index::SpatialSubscriptionIndex},
};
use andromeda_tenancy::tenant_ref::TenantRef;
use dashmap::DashMap;
use iri_rs::IriBuf;
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
    inner: DashMap<Uri, Arc<CompiledSubscription>>,
    by_type: DashMap<String, HashSet<Uri>>,
    untyped: dashmap::DashSet<Uri>,
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
                self.inner.insert(sub.id.clone(), Arc::new(compiled)); // clone: the map owns its key
            }
        }
        Ok(())
    }

    /// Handle a single subscription change event (upsert or removal).
    pub async fn handle_change(&self, change: &SubscriptionChange, repo: &impl SubscriptionStore) {
        match change {
            SubscriptionChange::Upserted(id) => {
                match repo.get(id.as_str()).await {
                    Ok(Some(sub)) => {
                        if sub.lifecycle.status == SubscriptionStatus::Active {
                            if let Some(compiled) = compile_subscription(&sub) {
                                self.index_remove(id);
                                self.index_insert(&compiled);
                                self.inner.insert(id.clone(), Arc::new(compiled)); // clone: the map owns its key
                            }
                        } else {
                            self.index_remove(id);
                            self.inner.remove(id);
                        }
                    }
                    Ok(None) => {
                        self.index_remove(id);
                        self.inner.remove(id);
                    }
                    Err(e) => {
                        debug!("Failed to load subscription {} for cache update: {}", id, e);
                    }
                }
            }
            SubscriptionChange::Deleted(id) => {
                self.index_remove(id);
                self.inner.remove(id);
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
        let mut candidate_ids: HashSet<Uri> = self.untyped.iter().map(|entry| entry.key().clone()).collect();
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
    pub fn get(&self, id: &Uri) -> Option<Arc<CompiledSubscription>> {
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
        // A selector whose type is a boolean expression can match several
        // expanded IRIs that the cache cannot enumerate ahead of time; it falls
        // back to the untyped bucket so `find_matching` still considers it.
        let mut indexed_any = false;
        for selector in &compiled.selectors {
            match selector.type_.as_single_type() {
                Some(type_iri) => {
                    self.by_type.entry(type_iri.to_owned()).or_default().insert(compiled.id.clone());
                }
                None => {
                    self.untyped.insert(compiled.id.clone());
                }
            }
            indexed_any = true;
        }
        if !indexed_any {
            self.untyped.insert(compiled.id.clone());
        }
    }

    fn index_remove(&self, id: &Uri) {
        self.spatial.remove(id);
        self.untyped.remove(id);
        // Drop the id from every type bucket it appears in. `retain` scans
        // the map once; the keys per bucket are short (one `HashSet<Uri>`
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
        // clone: the key owns its IRI, which the compiled subscription keeps
        let geoproperty_iri = geo_q.geoproperty_iri.clone();

        // clone: the entry owns its identifier, geometry and relation, all borrowed here
        let Some(entry) = SubEntry::new(compiled.id.clone(), geo_q.geometry.as_ref().clone(), geo_q.georel.clone()) else {
            debug!("Subscription {} spatial index skipped: geometry has no bounding rect", compiled.id);
            return;
        };

        // A concretely-typed selector indexes under every type it names. A
        // subscription with no selectors matches every type, and a selector
        // whose type is a boolean expression matches types the per-type
        // partitioning cannot enumerate; both are indexed under the wildcard
        // partition (shared with the notification worker) so the subscription
        // is still reached and the residual filter re-checks the real selector.
        // A selector mixing a concrete and a boolean type therefore gets both a
        // concrete entry and a wildcard one, rather than losing the boolean arm.
        let mut needs_wildcard = compiled.selectors.is_empty();
        for selector in &compiled.selectors {
            let Some(type_name) = selector.type_.as_single_type() else {
                needs_wildcard = true;
                continue;
            };
            match IriBuf::new(type_name.to_owned()) {
                Ok(type_iri) => {
                    let key = SpatialKey::new(self.tenant.clone(), type_iri, geoproperty_iri.clone());
                    self.spatial.insert(key, entry.clone());
                }
                Err(_) => {
                    debug!(
                        "Subscription {} spatial index selector type '{}' is not a valid IRI; indexing under the wildcard partition",
                        compiled.id, type_name
                    );
                    needs_wildcard = true;
                }
            }
        }
        if needs_wildcard {
            let key = SpatialKey::any_type(self.tenant.clone(), geoproperty_iri);
            self.spatial.insert(key, entry);
        }
    }
}

/// Compile a domain subscription into a pre-compiled form for fast matching.
///
/// Returns `None` if the subscription's endpoint transport cannot be parsed
/// (logged at debug level).
pub fn compile_subscription(sub: &Subscription) -> Option<CompiledSubscription> {
    // clone: the compiled form outlives the borrowed subscription and every
    // selector member is already parsed, so there is nothing left to compile
    let selectors: Vec<EntitySelector> = sub.selection.entities.clone().unwrap_or_default();

    let triggers: Vec<String> = sub.selection.notification_trigger.as_ref().map_or_else(
        || TriggerReason::defaults().iter().map(|s| String::from(*s)).collect(),
        |t| t.iter().map(|nt| nt.to_string()).collect(),
    );

    let watched_attrs = sub
        .selection
        .watched_attributes
        .as_ref()
        .map(|attrs| attrs.iter().map(ToString::to_string).collect::<HashSet<String>>());

    let endpoint_transport = match parse_endpoint_transport(sub.notification.endpoint.uri.as_str(), sub.notification.endpoint.notifier_info.as_deref()) {
        Ok(t) => t,
        Err(e) => {
            debug!("Failed to parse endpoint transport for subscription {}: {}", sub.id, e);
            return None;
        }
    };

    Some(CompiledSubscription {
        // clone: the compiled form outlives the borrowed subscription
        id: sub.id.clone(),
        status: sub.lifecycle.status,
        endpoint_uri: sub.notification.endpoint.uri.clone(),
        // clone: the compiled form outlives the subscription it was built from
        endpoint_accept: sub.notification.endpoint.accept.clone(),
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
        jsonld_context: sub.delivery.jsonld_context.clone(),
        expires_at: sub.lifecycle.expires_at,
        // clone: the compiled form owns its filters, and the subscription it was
        // built from is borrowed and outlives it
        compiled_q: sub.filters.q.clone(),
        compiled_geo_q: sub.filters.geo_q.as_ref().and_then(|v| compile_geo_q(v, sub.id.as_ref())),
        compiled_scope_q: sub.filters.scope_q.clone(),
        dataset_id: sub.filters.dataset_id.as_ref().map(|ids| ids.iter().map(ToString::to_string).collect()),
        notification_join: sub.notification.join.map(|j| j.to_string()),
        notification_join_level: sub.notification.join_level.unwrap_or(1).min(5),
        compiled_csf: sub.filters.csf.clone(),
        local_only: sub.delivery.local_only.unwrap_or(false),
    })
}

/// Pre-parse a subscription's stored `geoQ` into the same [`GeoQuery`] a
/// request-borne geo filter compiles to (clause 4.10).
///
/// Returns `None` when any member is missing or malformed, logged at debug
/// level. The subscription then never matches through `geoQ` at all, which is
/// the same answer re-parsing on every event would give and costs nothing per
/// event to reach.
pub fn compile_geo_q(geo_q: &serde_json::Value, sub_id: &str) -> Option<GeoQuery> {
    let geo_obj = geo_q.as_object()?;
    let georel_str = geo_obj.get("georel").and_then(serde_json::Value::as_str)?;
    let geometry_str = geo_obj.get("geometry").and_then(serde_json::Value::as_str)?;
    let coordinates = match geo_obj.get("coordinates")? {
        serde_json::Value::String(raw) => serde_json::from_str::<serde_json::Value>(raw).ok()?,
        // clone: the compiled query owns its reference geometry
        other => other.clone(),
    };
    let geoproperty_iri = match geo_obj.get("geoproperty").and_then(serde_json::Value::as_str) {
        Some(raw) => match IriBuf::new(raw.to_owned()) {
            Ok(iri) => iri,
            Err(_) => {
                debug!("geoQ for subscription {} names a geoproperty that is not an IRI: '{}'", sub_id, raw);
                return None;
            }
        },
        None => expanded_properties::LOCATION.into(),
    };

    let geometry = match Geometry::from_geojson_value(&serde_json::json!({ "type": geometry_str, "coordinates": coordinates })) {
        Ok(geometry) => geometry,
        Err(error) => {
            debug!("Failed to parse geoQ geometry for subscription {}: {}", sub_id, error);
            return None;
        }
    };

    Some(GeoQuery {
        georel: georel_str.parse().ok()?,
        geometry,
        geoproperty_iri,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_delivery::endpoint::transport::EndpointTransport;
    use andromeda_ngsi_ld_model::subscription::SubscriptionStatus;
    use url::Url;

    fn make_compiled_sub(id: &str, type_: &str, triggers: Vec<&str>) -> CompiledSubscription {
        CompiledSubscription {
            id: format!("urn:ngsi-ld:Subscription:{id}").parse().expect("a valid subscription URI"),
            status: SubscriptionStatus::Active,
            endpoint_uri: Url::parse("http://example.com/notify").expect("a valid endpoint URI"),
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_transport: EndpointTransport::Http,
            triggers: triggers.into_iter().map(String::from).collect(),
            watched_attrs: None,
            selectors: vec![EntitySelector {
                id: None,
                id_pattern: None,
                type_: type_.parse().expect("valid type selection expression"),
            }],
            throttling_secs: None,
            time_interval_secs: None,
            notification_attributes: None,
            notification_format: None,
            notification_sys_attrs: andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion::Strip,
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

        let sub1 = make_compiled_sub("sub1", "Vehicle", vec!["entityCreated"]);
        cache.inner.insert(sub1.id.clone(), Arc::new(sub1)); // clone: the map owns its key
        assert_eq!(cache.len(), 1);

        let sub2 = make_compiled_sub("sub2", "Building", vec!["entityCreated"]);
        cache.inner.insert(sub2.id.clone(), Arc::new(sub2)); // clone: the map owns its key
        assert_eq!(cache.len(), 2);
    }
}
