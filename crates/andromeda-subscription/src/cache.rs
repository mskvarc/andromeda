//! Shared subscription cache for fast in-memory matching.
//!
//! One instance per tenant, used by both the notification worker (event
//! matching) and the entity service (notification-event generation). The
//! compiled subscriptions and every routing bucket live in a
//! [`CandidateIndex`] behind a single read-write lock; the geo index is the
//! shared [`SpatialSubscriptionIndex`] the spatial narrowing consults.

use crate::{
    candidate_funnel::MatchOutcome,
    candidate_index::CandidateIndex,
    change::SubscriptionChange,
    compilation::compile_subscription,
    compiled::CompiledSubscription,
    error::SubscriptionError,
    store::SubscriptionStore,
};
use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_context_space::{context_space::ContextSpaceKind, context_space_key::ContextSpaceKey, context_target::ContextTarget};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::subscription::SubscriptionStatus;
use andromeda_spatial_index::{
    key::SpatialKey,
    subscription::{entry::SubEntry, index::SpatialSubscriptionIndex},
};
use andromeda_tenancy::tenant_ref::TenantRef;
use iri_rs::IriBuf;
use parking_lot::RwLock;
use std::{collections::HashMap, sync::Arc};
use tracing::debug;

/// Thread-safe subscription cache for fast event matching.
///
/// Reads take the index lock shared; a matching pass therefore sees one
/// consistent generation of every bucket. Writes are rare (subscription
/// lifecycle changes) and take it exclusively.
#[derive(Debug)]
pub struct SubscriptionCache {
    indices: RwLock<HashMap<ContextSpaceKey, CandidateIndex>>,
    spatial: Arc<SpatialSubscriptionIndex>,
    tenant: TenantRef,
}

impl SubscriptionCache {
    /// Create an empty subscription cache scoped to the given tenant.
    #[must_use]
    pub fn new(tenant: TenantRef) -> SubscriptionCache {
        SubscriptionCache::with_spatial(tenant, Arc::new(SpatialSubscriptionIndex::new()))
    }

    /// Create an empty subscription cache that writes its geo index into
    /// the given shared [`SpatialSubscriptionIndex`].
    pub fn with_spatial(tenant: TenantRef, spatial: Arc<SpatialSubscriptionIndex>) -> SubscriptionCache {
        SubscriptionCache {
            indices: RwLock::new(HashMap::new()),
            spatial,
            tenant,
        }
    }

    /// Reload the entire cache from the subscription repository.
    ///
    /// Clears existing entries and replaces them with all currently active
    /// subscriptions. Failed compilations are logged and skipped.
    pub async fn reload(&self, target: &ContextTarget, repo: &(impl SubscriptionStore + ?Sized)) -> Result<(), SubscriptionError> {
        let subscriptions = repo.list_active(target).await?;
        let compiled: Vec<CompiledSubscription> = subscriptions.iter().map(compile_subscription).collect();
        let mut indices = self.indices.write();
        let index = indices.entry(target.key()).or_default();
        index.clear();
        for compiled in compiled {
            self.spatial_insert(target, &compiled);
            index.insert(Arc::new(compiled));
        }
        Ok(())
    }

    /// Handle a single subscription change event (upsert or removal).
    pub async fn handle_change(&self, change: &SubscriptionChange, repo: &(impl SubscriptionStore + ?Sized)) {
        match change {
            SubscriptionChange::Upserted { target, id } => match repo.get(target, id).await {
                Ok(Some(stored)) => {
                    if stored.subscription.lifecycle.status == SubscriptionStatus::Active {
                        self.insert(target, compile_subscription(&stored));
                    } else {
                        self.remove(target, id);
                    }
                }
                Ok(None) => self.remove(target, id),
                Err(e) => {
                    debug!("Failed to load subscription {} for cache update: {}", id, e);
                }
            },
            SubscriptionChange::Deleted { target, id } => self.remove(target, id),
        }
    }

    /// Index one compiled subscription, replacing any earlier entry with the
    /// same identifier in both the candidate and the spatial index.
    pub fn insert(&self, target: &ContextTarget, compiled: CompiledSubscription) {
        if target.kind() == ContextSpaceKind::Live {
            self.spatial.remove(&compiled.id);
            self.spatial_insert(target, &compiled);
        }
        self.indices.write().entry(target.key()).or_default().insert(Arc::new(compiled));
    }

    /// Drop the subscription with this identifier from both indexes.
    pub fn remove(&self, target: &ContextTarget, id: &Uri) {
        if target.kind() == ContextSpaceKind::Live {
            self.spatial.remove(id);
        }
        if let Some(index) = self.indices.write().get_mut(&target.key()) {
            index.remove(id);
        }
    }

    /// Find all compiled subscriptions that match the given change event.
    ///
    /// Routes the event through the candidate funnel (selectors, trigger,
    /// watched attributes) and evaluates the authoritative matcher only on
    /// the survivors. The outcome carries the funnel counts for the caller to
    /// log and record.
    pub fn find_matching(&self, target: &ContextTarget, event: &ChangeEvent) -> MatchOutcome {
        self.indices
            .read()
            .get(&target.key())
            .map_or_else(MatchOutcome::default, |index| index.find_matching(event))
    }

    /// Look up a single compiled subscription by ID.
    pub fn get(&self, target: &ContextTarget, id: &Uri) -> Option<Arc<CompiledSubscription>> {
        self.indices.read().get(&target.key()).and_then(|index| index.get(id).map(Arc::clone))
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

    /// Returns the number of cached subscriptions.
    pub fn len(&self, target: &ContextTarget) -> usize {
        self.indices.read().get(&target.key()).map_or(0, CandidateIndex::len)
    }

    /// Whether the cache contains no compiled subscriptions.
    pub fn is_empty(&self, target: &ContextTarget) -> bool {
        self.len(target) == 0
    }

    fn spatial_insert(&self, target: &ContextTarget, compiled: &CompiledSubscription) {
        if target.kind() != ContextSpaceKind::Live {
            return;
        }
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
            if let Ok(type_iri) = IriBuf::new(type_name.as_str().to_owned()) {
                let key = SpatialKey::new(self.tenant.clone(), type_iri, geoproperty.clone());
                self.spatial.insert(key, entry.clone());
            } else {
                debug!(
                    "Subscription {} spatial index selector type '{}' is not a valid IRI; indexing under the wildcard partition",
                    compiled.id, type_name
                );
                needs_wildcard = true;
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
    use crate::error::SubscriptionError;
    use andromeda_context_commit::subscription_key::SubscriptionKey;
    use andromeda_context_space::context_space::ContextSpace;
    use andromeda_ngsi_ld_model::subscription::{EntitySelector, SubscriptionStatus};
    use andromeda_ngsi_ld_query::projection::compiled::Projection;
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use chrono::{DateTime, Utc};
    use std::num::NonZeroU32;
    use url::Url;

    fn live_target() -> ContextTarget {
        let key = ContextSpaceKey::try_from(1).expect("positive context-space key");
        ContextTarget::live(ContextSpace::new(key, ContextSpaceKind::Live)).expect("live context target")
    }

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
            notification_projection: Projection::default(),
        })
    }

    #[test]
    fn len_reflects_cache_size() -> Result<(), SubscriptionError> {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault);
        let target = live_target();
        assert!(cache.is_empty(&target));
        assert_eq!(cache.len(&target), 0);

        let sub1 = make_compiled_sub("sub1", 1, "Vehicle", vec!["entityCreated"])?;
        cache.insert(&target, sub1);
        assert_eq!(cache.len(&target), 1);

        let sub2 = make_compiled_sub("sub2", 2, "Building", vec!["entityCreated"])?;
        cache.insert(&target, sub2);
        assert_eq!(cache.len(&target), 2);
        Ok(())
    }

    #[test]
    fn a_cached_subscription_carries_its_store_key_beside_its_identifier() -> Result<(), SubscriptionError> {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault);
        let target = live_target();
        let subscription = make_compiled_sub("keyed", 7, "Vehicle", vec!["entityCreated"])?;
        let id = subscription.id.clone(); // clone: the lookup outlives the value the map takes
        cache.insert(&target, subscription);

        assert_eq!(cache.get(&target, &id).map(|compiled| compiled.key), SubscriptionKey::try_from(7).ok());
        Ok(())
    }
}
