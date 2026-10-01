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
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
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
    indices: RwLock<HashMap<ContextTarget, CandidateIndex>>,
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
    ///
    /// # Errors
    ///
    /// Returns the store's failure when the active subscriptions of `target`
    /// cannot be listed — [`SubscriptionError::Persistence`] for an adapter
    /// failure, [`SubscriptionError::Representation`] for a stored row that no
    /// longer reads back as a subscription. The cache is left untouched in
    /// either case.
    pub async fn reload(&self, target: &ContextTarget, repo: &(impl SubscriptionStore + ?Sized)) -> Result<(), SubscriptionError> {
        let subscriptions = repo.list_active(target).await?;
        let compiled: Vec<CompiledSubscription> = subscriptions.iter().map(compile_subscription).collect();
        let mut indices = self.indices.write();
        let index = indices.entry(target.clone()).or_default();
        index.clear();
        for compiled in compiled {
            self.spatial_insert(target, &compiled);
            index.insert(Arc::new(compiled));
        }
        Ok(())
    }

    /// Handle a single subscription change event (upsert or removal).
    ///
    /// A subscription that cannot be re-read is logged and left as cached: the
    /// change stream offers no retry, and the next change, reload or refresh
    /// of the subscription reconciles it.
    pub async fn handle_change(&self, change: &SubscriptionChange, repo: &(impl SubscriptionStore + ?Sized)) {
        match change {
            SubscriptionChange::Upserted { target, id } => {
                if let Err(error) = self.refresh(target, id, repo).await {
                    debug!("Failed to load subscription {} for cache update: {}", id, error);
                }
            }
            SubscriptionChange::Deleted { target, id } => self.remove(target, id),
        }
    }

    /// Re-read one subscription from the store and index it as it now stands:
    /// compiled while it is active, dropped once it is not or no longer exists.
    ///
    /// # Errors
    ///
    /// Returns the store's failure when the subscription cannot be read —
    /// [`SubscriptionError::Persistence`] for an adapter failure,
    /// [`SubscriptionError::Representation`] for a stored row that no longer
    /// reads back as a subscription. The cached entry is left as it was, so a
    /// caller can tell a subscription that is gone from one it could not see.
    pub async fn refresh(&self, target: &ContextTarget, id: &Identifier, repo: &(impl SubscriptionStore + ?Sized)) -> Result<(), SubscriptionError> {
        match repo.get(target, id).await? {
            Some(stored) if stored.subscription.lifecycle.status == SubscriptionStatus::Active => self.insert(target, compile_subscription(&stored)),
            Some(_) | None => self.remove(target, id),
        }
        Ok(())
    }

    /// Index one compiled subscription, replacing any earlier entry with the
    /// same identifier in both the candidate and the spatial index.
    pub fn insert(&self, target: &ContextTarget, compiled: CompiledSubscription) {
        if target.snapshot().is_none() {
            self.spatial.remove(&compiled.id);
            self.spatial_insert(target, &compiled);
        }
        self.indices.write().entry(target.clone()).or_default().insert(Arc::new(compiled));
    }

    /// Drop the subscription with this identifier from both indexes.
    pub fn remove(&self, target: &ContextTarget, id: &Identifier) {
        if target.snapshot().is_none() {
            self.spatial.remove(id);
        }
        if let Some(index) = self.indices.write().get_mut(target) {
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
            .get(target)
            .map_or_else(MatchOutcome::default, |index| index.find_matching(event))
    }

    /// Look up a single compiled subscription by ID.
    pub fn get(&self, target: &ContextTarget, id: &Identifier) -> Option<Arc<CompiledSubscription>> {
        self.indices.read().get(target).and_then(|index| index.get(id).map(Arc::clone))
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
        self.indices.read().get(target).map_or(0, CandidateIndex::len)
    }

    /// Whether the cache contains no compiled subscriptions.
    pub fn is_empty(&self, target: &ContextTarget) -> bool {
        self.len(target) == 0
    }

    fn spatial_insert(&self, target: &ContextTarget, compiled: &CompiledSubscription) {
        if target.snapshot().is_some() {
            return;
        }
        let Some(geo_q) = compiled.compiled_geo_q.restriction() else {
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
    use crate::{
        cache::SubscriptionCache,
        compiled::CompiledSubscription,
        error::SubscriptionError,
        geo_filter::SubscriptionGeoFilter,
        notification_slot::{NotificationSlot, NotificationSlotGate},
        store::{SubscriptionFields, SubscriptionStore},
        stored_subscription::StoredSubscription,
    };
    use andromeda_context_commit::subscription_key::SubscriptionKey;
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::subscription::{EntitySelector, Subscription, SubscriptionStatus};
    use andromeda_ngsi_ld_query::{cursor::PageCursor, projection::compiled::Projection};
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_tenancy::tenant_ref::TenantRef;
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU32;
    use url::Url;

    /// What the store double answers when a subscription is read back.
    #[derive(Clone, Copy)]
    enum StoredAnswer {
        /// The store fails the read, as an unreachable database would.
        Unreadable,
        /// The store holds no such subscription.
        Absent,
    }

    /// A subscription store that answers only `get`, as `answer` says.
    struct AnsweringStore {
        answer: StoredAnswer,
    }

    fn unused(method: &str) -> SubscriptionError {
        panic!("{}", format!("AnsweringStore::{method} is not exercised by the cache refresh tests"))
    }

    #[async_trait]
    impl SubscriptionStore for AnsweringStore {
        async fn get(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<StoredSubscription>, SubscriptionError> {
            match self.answer {
                StoredAnswer::Unreadable => Err(SubscriptionError::from(PersistenceFailure::Unavailable)),
                StoredAnswer::Absent => Ok(None),
            }
        }

        async fn list(&self, _target: &ContextTarget, _limit: Option<i64>, _offset: Option<i64>) -> Result<(Vec<Subscription>, i64), SubscriptionError> {
            Err(unused("list"))
        }

        async fn list_keyset(&self, _target: &ContextTarget, _cursor: Option<&PageCursor>, _limit: i64) -> Result<(Vec<Subscription>, i64), SubscriptionError> {
            Err(unused("list_keyset"))
        }

        async fn delete(&self, _target: &ContextTarget, _id: &Identifier) -> Result<u64, SubscriptionError> {
            Err(unused("delete"))
        }

        async fn exists(&self, _target: &ContextTarget, _id: &Identifier) -> Result<bool, SubscriptionError> {
            Err(unused("exists"))
        }

        async fn list_active(&self, _target: &ContextTarget) -> Result<Vec<StoredSubscription>, SubscriptionError> {
            Err(unused("list_active"))
        }

        async fn claim_notification_slot(
            &self,
            _target: &ContextTarget,
            _id: &Identifier,
            _gate: NotificationSlotGate,
        ) -> Result<NotificationSlot, SubscriptionError> {
            Err(unused("claim_notification_slot"))
        }

        async fn mark_success(&self, _target: &ContextTarget, _id: &Identifier) -> Result<(), SubscriptionError> {
            Err(unused("mark_success"))
        }

        async fn mark_failure(&self, _target: &ContextTarget, _id: &Identifier) -> Result<(), SubscriptionError> {
            Err(unused("mark_failure"))
        }

        async fn expire_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<Identifier>, SubscriptionError> {
            Err(unused("expire_due"))
        }

        async fn find_due_interval_subscriptions(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<Subscription>, SubscriptionError> {
            Err(unused("find_due_interval_subscriptions"))
        }

        async fn next_interval_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, SubscriptionError> {
            Err(unused("next_interval_due"))
        }

        async fn advance_interval_anchor(&self, _target: &ContextTarget, _id: &Identifier, _cutoff: DateTime<Utc>) -> Result<(), SubscriptionError> {
            Err(unused("advance_interval_anchor"))
        }

        async fn create_subscription(
            &self,
            _target: &ContextTarget,
            _id: &Identifier,
            _fields: &SubscriptionFields<'_>,
            _status: SubscriptionStatus,
        ) -> Result<(), SubscriptionError> {
            Err(unused("create_subscription"))
        }

        async fn update_subscription(
            &self,
            _target: &ContextTarget,
            _id: &Identifier,
            _fields: &SubscriptionFields<'_>,
            _status: SubscriptionStatus,
        ) -> Result<u64, SubscriptionError> {
            Err(unused("update_subscription"))
        }
    }

    fn live_target() -> ContextTarget {
        ContextTarget::Live
    }

    fn make_compiled_sub(id: &str, key: i64, type_: &str, triggers: Vec<&str>) -> CompiledSubscription {
        CompiledSubscription {
            id: format!("urn:ngsi-ld:Subscription:{id}").parse().expect("invalid fixture subscription URI:"),
            key: SubscriptionKey::try_from(key).expect("invalid fixture subscription key:"),
            status: SubscriptionStatus::Active,
            endpoint_uri: Url::parse("http://example.com/notify").expect("invalid fixture endpoint URI:"),
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_notifier_info: None,
            endpoint_cooldown: None,
            endpoint_timeout: None,
            triggers: triggers
                .into_iter()
                .map(|trigger| trigger.parse().unwrap_or_else(|error| panic!("invalid fixture trigger: {error}")))
                .collect(),
            watched_attrs: None,
            selectors: vec![EntitySelector {
                id: None,
                id_pattern: None,
                type_: type_.parse().expect("invalid fixture type expression:"),
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
            compiled_geo_q: SubscriptionGeoFilter::Unrestricted,
            compiled_scope_q: None,
            dataset_id: None,
            notification_join: None,
            notification_join_level: NonZeroU32::MIN,
            compiled_csf: None,
            local_only: false,
            ngsild_conformance: None,
            notification_projection: Projection::default(),
        }
    }

    #[test]
    fn len_reflects_cache_size() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault);
        let target = live_target();
        assert!(cache.is_empty(&target));
        assert_eq!(cache.len(&target), 0);

        let sub1 = make_compiled_sub("sub1", 1, "Vehicle", vec!["entityCreated"]);
        cache.insert(&target, sub1);
        assert_eq!(cache.len(&target), 1);

        let sub2 = make_compiled_sub("sub2", 2, "Building", vec!["entityCreated"]);
        cache.insert(&target, sub2);
        assert_eq!(cache.len(&target), 2);
    }

    #[test]
    fn a_cached_subscription_carries_its_store_key_beside_its_identifier() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault);
        let target = live_target();
        let subscription = make_compiled_sub("keyed", 7, "Vehicle", vec!["entityCreated"]);
        let id = subscription.id.clone(); // clone: the lookup outlives the value the map takes
        cache.insert(&target, subscription);

        assert_eq!(cache.get(&target, &id).map(|compiled| compiled.key), SubscriptionKey::try_from(7).ok());
    }

    /// A subscription the store could not be asked about is not a subscription
    /// the store no longer holds: the failure is reported, and the cached entry
    /// stays so a caller that retries still finds it.
    #[tokio::test]
    async fn a_refresh_the_store_cannot_answer_reports_the_failure_and_keeps_the_entry() -> Result<(), SubscriptionError> {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault);
        let target = live_target();
        let subscription = make_compiled_sub("unreadable", 3, "Vehicle", vec!["entityCreated"]);
        let id = subscription.id.clone(); // clone: the lookup outlives the value the map takes
        cache.insert(&target, subscription);

        let refreshed = cache
            .refresh(
                &target,
                &id,
                &AnsweringStore {
                    answer: StoredAnswer::Unreadable,
                },
            )
            .await;

        assert!(matches!(refreshed, Err(SubscriptionError::StoreUnavailable)), "{refreshed:?}");
        assert!(cache.get(&target, &id).is_some(), "an unanswered refresh must not drop the cached subscription");
        Ok(())
    }

    /// A subscription the store no longer holds is dropped from the cache.
    #[tokio::test]
    async fn a_refresh_that_finds_the_subscription_gone_drops_it() -> Result<(), SubscriptionError> {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault);
        let target = live_target();
        let subscription = make_compiled_sub("gone", 4, "Vehicle", vec!["entityCreated"]);
        let id = subscription.id.clone(); // clone: the lookup outlives the value the map takes
        cache.insert(&target, subscription);

        cache.refresh(&target, &id, &AnsweringStore { answer: StoredAnswer::Absent }).await?;

        assert!(cache.get(&target, &id).is_none());
        Ok(())
    }
}
