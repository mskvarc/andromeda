//! Shared subscription cache for fast in-memory matching.
//!
//! Extracts the compilation and caching logic formerly owned by
//! `NotificationWorker` into a shared, concurrency-safe structure
//! backed by `DashMap`. Used by both the notification worker (event
//! matching) and the entity service (notification-event generation).

use crate::{
    change::SubscriptionChange,
    compilation::compile_subscription,
    compiled::CompiledSubscription,
    error::SubscriptionError,
    matching::find_matching,
    store::SubscriptionStore,
};
use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::subscription::SubscriptionStatus;
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
    by_type: DashMap<EntityTypeName, HashSet<Uri>>,
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
    pub async fn reload(&self, repo: &(impl SubscriptionStore + ?Sized)) -> Result<(), SubscriptionError> {
        let subscriptions = repo.list_active().await?;
        self.inner.clear();
        self.by_type.clear();
        self.untyped.clear();
        for stored in subscriptions {
            let compiled = compile_subscription(&stored);
            self.index_insert(&compiled);
            self.inner.insert(stored.subscription.id.clone(), Arc::new(compiled)); // clone: the map owns its key
        }
        Ok(())
    }

    /// Handle a single subscription change event (upsert or removal).
    pub async fn handle_change(&self, change: &SubscriptionChange, repo: &(impl SubscriptionStore + ?Sized)) {
        match change {
            SubscriptionChange::Upserted(id) => {
                match repo.get(id).await {
                    Ok(Some(stored)) => {
                        if stored.subscription.lifecycle.status == SubscriptionStatus::Active {
                            let compiled = compile_subscription(&stored);
                            self.index_remove(id);
                            self.index_insert(&compiled);
                            self.inner.insert(id.clone(), Arc::new(compiled)); // clone: the map owns its key
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
            if let Some(entry) = self.by_type.get(event_type) {
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
                Some(entity_type) => {
                    self.by_type.entry(entity_type.clone()).or_default().insert(compiled.id.clone());
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

    /// Whether the cache contains no compiled subscriptions.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn spatial_insert(&self, compiled: &CompiledSubscription) {
        let Some(geo_q) = compiled.compiled_geo_q.as_ref() else {
            return;
        };
        // clone: the key owns its name, which the compiled subscription keeps
        let geoproperty = geo_q.geoproperty.clone();

        // clone: the entry owns its identifier, geometry and relation, all borrowed here
        let Some(entry) = SubEntry::new(compiled.id.clone(), geo_q.geometry.clone(), geo_q.georel.clone()) else {
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
            match IriBuf::new(type_name.as_str().to_owned()) {
                Ok(type_iri) => {
                    let key = SpatialKey::new(self.tenant.clone(), type_iri, geoproperty.clone());
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
            let key = SpatialKey::any_type(self.tenant.clone(), geoproperty);
            self.spatial.insert(key, entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{error::SubscriptionError, subscription_key::SubscriptionKey};
    use andromeda_ngsi_ld_model::subscription::{EntitySelector, SubscriptionStatus};
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use chrono::{DateTime, Utc};
    use std::num::NonZeroU32;
    use url::Url;

    fn make_compiled_sub(id: &str, key: i64, type_: &str, triggers: Vec<&str>) -> Result<CompiledSubscription, SubscriptionError> {
        Ok(CompiledSubscription {
            id: format!("urn:ngsi-ld:Subscription:{id}")
                .parse()
                .map_err(|error| SubscriptionError::internal(format!("invalid fixture subscription URI: {error}")))?,
            key: SubscriptionKey::try_from(key).map_err(|error| SubscriptionError::internal(format!("invalid fixture subscription key: {error}")))?,
            status: SubscriptionStatus::Active,
            endpoint_uri: Url::parse("http://example.com/notify")
                .map_err(|error| SubscriptionError::internal(format!("invalid fixture endpoint URI: {error}")))?,
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_notifier_info: None,
            triggers: triggers
                .into_iter()
                .map(|trigger| {
                    trigger
                        .parse()
                        .map_err(|error| SubscriptionError::internal(format!("invalid fixture trigger: {error}")))
                })
                .collect::<Result<Vec<_>, _>>()?,
            watched_attrs: None,
            selectors: vec![EntitySelector {
                id: None,
                id_pattern: None,
                type_: type_
                    .parse()
                    .map_err(|error| SubscriptionError::internal(format!("invalid fixture type expression: {error}")))?,
            }],
            throttling_secs: None,
            time_interval_secs: None,
            notification_attributes: None,
            notification_format: None,
            notification_sys_attrs: SysAttrsInclusion::Strip,
            notification_show_changes: false,
            jsonld_context: None,
            expires_at: None,
            created_at: DateTime::<Utc>::MIN_UTC,
            compiled_q: None,
            compiled_geo_q: None,
            compiled_scope_q: None,
            dataset_id: None,
            notification_join: None,
            notification_join_level: NonZeroU32::MIN,
            compiled_csf: None,
            local_only: false,
        })
    }

    #[test]
    fn len_reflects_cache_size() -> Result<(), SubscriptionError> {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault);
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);

        let sub1 = make_compiled_sub("sub1", 1, "Vehicle", vec!["entityCreated"])?;
        cache.inner.insert(sub1.id.clone(), Arc::new(sub1)); // clone: the map owns its key
        assert_eq!(cache.len(), 1);

        let sub2 = make_compiled_sub("sub2", 2, "Building", vec!["entityCreated"])?;
        cache.inner.insert(sub2.id.clone(), Arc::new(sub2)); // clone: the map owns its key
        assert_eq!(cache.len(), 2);
        Ok(())
    }

    #[test]
    fn a_cached_subscription_carries_its_store_key_beside_its_identifier() -> Result<(), SubscriptionError> {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault);
        let subscription = make_compiled_sub("keyed", 7, "Vehicle", vec!["entityCreated"])?;
        let id = subscription.id.clone(); // clone: the lookup outlives the value the map takes
        cache.inner.insert(id.clone(), Arc::new(subscription)); // clone: the map owns its key

        assert_eq!(cache.get(&id).map(|compiled| compiled.key), SubscriptionKey::try_from(7).ok());
        Ok(())
    }
}
