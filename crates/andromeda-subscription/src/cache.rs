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
    refresh_order::RefreshOrder,
    store::SubscriptionStore,
};
use andromeda_context_commit::{change_event::ChangeEvent, store_instant::StoreInstant};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::filter::language_matching::LanguageMatching;
use andromeda_retry::{growth::Growth, retry_delay::RetryDelay, retry_policy::RetryPolicy, schedule::BackoffSchedule};
use andromeda_spatial_index::{
    key::SpatialKey,
    subscription::{entry::SubEntry, index::SpatialSubscriptionIndex},
};
use andromeda_tenancy::tenant_ref::TenantRef;
use chrono::Utc;
use iri_rs::IriBuf;
use parking_lot::RwLock;
use std::{collections::HashMap, num::NonZeroU32, sync::Arc, time::Duration};
use tracing::{debug, error, warn};

/// How often, and how far apart, one reconcile pass reads a subscription whose
/// store could not be reached: four reads, waiting 100, 200 and 400
/// milliseconds.
const REFRESH_RETRY: RetryPolicy = RetryPolicy {
    attempts: NonZeroU32::MIN.saturating_add(3),
    schedule: BackoffSchedule::new(Duration::from_millis(100), Duration::from_millis(400), Growth::Doubling),
};

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
    /// The broker's mode every cached subscription's target languages select
    /// keys by.
    language_matching: LanguageMatching,
    /// The order the refreshes of each subscription take effect in, and the
    /// subscriptions whose newest refresh could not reach the store: their
    /// cached entry, if any, may be stale, and
    /// [`SubscriptionCache::reconcile_dirty`] reads them again.
    refreshes: RefreshOrder,
}

impl SubscriptionCache {
    /// Create an empty subscription cache scoped to the given tenant, its
    /// subscriptions' target languages selecting keys by `language_matching`.
    #[must_use]
    pub fn new(tenant: TenantRef, language_matching: LanguageMatching) -> SubscriptionCache {
        SubscriptionCache::with_spatial(tenant, Arc::new(SpatialSubscriptionIndex::new()), language_matching)
    }

    /// Create an empty subscription cache that writes its geo index into
    /// the given shared [`SpatialSubscriptionIndex`].
    pub fn with_spatial(tenant: TenantRef, spatial: Arc<SpatialSubscriptionIndex>, language_matching: LanguageMatching) -> SubscriptionCache {
        SubscriptionCache {
            indices: RwLock::new(HashMap::new()),
            spatial,
            tenant,
            language_matching,
            refreshes: RefreshOrder::default(),
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
        let compiled: Vec<CompiledSubscription> = subscriptions
            .iter()
            .map(|stored| compile_subscription(stored, self.language_matching))
            .collect();
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
    /// An upsert makes one read, as [`SubscriptionCache::refresh`] does. When
    /// the store cannot be reached the subscription is marked dirty for
    /// [`SubscriptionCache::reconcile_dirty`] to read again, because the
    /// change stream offers no retry and the caller is a request that must not
    /// wait on the store. A removal takes effect after every refresh of the
    /// subscription already begun, so none of them can index it again.
    ///
    /// # Errors
    ///
    /// Returns the read failure, as [`SubscriptionCache::refresh`] reports it.
    /// The cached entry is left as it was.
    pub async fn handle_change(&self, change: &SubscriptionChange, repo: &(impl SubscriptionStore + ?Sized)) -> Result<(), SubscriptionError> {
        match change {
            SubscriptionChange::Upserted { target, id } => self.refresh(target, id, repo).await,
            SubscriptionChange::Deleted { target, id } => {
                self.refreshes.begin(target, id).answered(|| self.remove(target, id));
                Ok(())
            }
        }
    }

    /// Re-read one subscription from the store and index it as it now stands:
    /// compiled while it is notified, dropped once it is not or no longer
    /// exists.
    ///
    /// A read whose store cannot be reached marks the subscription dirty; an
    /// answer, or a fault no later read is expected to mend, clears the mark.
    /// Refreshes of one subscription run concurrently and take effect in the
    /// order they began, so a slow read neither overwrites a newer answer nor
    /// clears the mark a newer failed read set.
    ///
    /// # Errors
    ///
    /// Returns the store's failure when the subscription cannot be read:
    /// [`SubscriptionError::Persistence`] for an adapter failure,
    /// [`SubscriptionError::Representation`] for a stored row that no longer
    /// reads back as a subscription. The cached entry is left as it was, so a
    /// caller can tell a subscription that is gone from one it could not see.
    pub async fn refresh(&self, target: &ContextTarget, id: &Identifier, repo: &(impl SubscriptionStore + ?Sized)) -> Result<(), SubscriptionError> {
        let read = self.refreshes.begin(target, id);
        match repo.get(target, id).await {
            Ok(stored) => {
                let notified = stored
                    .filter(|stored| stored.subscription.lifecycle.standing.notifies_at(Utc::now()))
                    .map(|stored| compile_subscription(&stored, self.language_matching));
                read.answered(|| match notified {
                    Some(compiled) => self.insert(target, compiled),
                    None => self.remove(target, id),
                });
                Ok(())
            }
            Err(error) => {
                read.failed(&error);
                Err(error)
            }
        }
    }

    /// Read every dirty subscription again, each through [`REFRESH_RETRY`]
    /// while the store cannot be reached.
    ///
    /// A subscription whose store stays out of reach stays dirty for the next
    /// pass. One whose read fails with a fault no retry mends (an unreadable
    /// stored row) is read once and no longer dirty: it stays as cached until
    /// it changes again or the cache is reloaded.
    pub async fn reconcile_dirty(&self, repo: &(impl SubscriptionStore + ?Sized), delay: &dyn RetryDelay) {
        for (target, id) in self.refreshes.unreachable() {
            let outcome = REFRESH_RETRY
                .run(
                    delay,
                    |error: &SubscriptionError, retry| {
                        warn!("Failed to load subscription {} for cache reconcile (retry {}): {}", id, retry, error);
                        error.is_transient()
                    },
                    async |_| self.refresh(&target, &id, repo).await,
                )
                .await;
            match outcome {
                Ok(()) => {}
                Err(error) if error.is_transient() => warn!("Subscription {} stays as cached until the next reconcile: {}", id, error),
                Err(error) => error!(
                    "Subscription {} cannot be read and is no longer reconciled; it stays as cached until it changes or the cache reloads: {}",
                    id, error
                ),
            }
        }
    }

    /// Number of subscriptions awaiting [`SubscriptionCache::reconcile_dirty`].
    pub fn dirty_len(&self) -> usize {
        self.refreshes.unreachable_len()
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

    /// Find every compiled subscription a live change committed at
    /// `committed_at` matches under the selection that stood at its commit
    /// (clause 5.8.6): one in force, as [`SubscriptionCache::find_matching`]
    /// finds it, whose selection began at or before the commit, and each
    /// superseded selection that stood then.
    ///
    /// Both are read under one lock, so an update applied meanwhile neither
    /// hides the version that stood at the commit nor offers it twice.
    pub fn find_matching_committed_at(&self, target: &ContextTarget, event: &ChangeEvent, committed_at: StoreInstant) -> MatchOutcome {
        self.indices.read().get(target).map_or_else(MatchOutcome::default, |index| {
            let mut outcome = index.find_matching(event);
            outcome.matched.retain(|in_force| in_force.notifying_since.selected <= committed_at);
            outcome.matched.extend(index.find_superseded_matching(event, committed_at));
            outcome
        })
    }

    /// Whether `subscription` is the version in force in `target` rather than
    /// a selection an update superseded.
    pub fn is_in_force(&self, target: &ContextTarget, subscription: &CompiledSubscription) -> bool {
        self.indices.read().get(target).is_some_and(|index| index.is_in_force(subscription))
    }

    /// Drop every superseded selection no change still to be evaluated can
    /// have committed under: none is when `floor`, the earliest commit instant
    /// evaluation still holds, is at or after the selection's end, or when
    /// evaluation holds no change (`None`).
    ///
    /// The exclusive lock is taken only when a selection can be dropped.
    pub fn release_superseded(&self, floor: Option<StoreInstant>) {
        if !self.indices.read().values().any(|index| index.releases_superseded(floor)) {
            return;
        }
        for index in self.indices.write().values_mut() {
            index.release_superseded(floor);
        }
    }

    /// Number of superseded selections kept for `target`.
    pub fn superseded_len(&self, target: &ContextTarget) -> usize {
        self.indices.read().get(target).map_or(0, CandidateIndex::superseded_len)
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

        // clone: the entry owns its identifier and relation, both borrowed here
        let Some(entry) = SubEntry::new(compiled.id.clone(), geo_q.geometry.form(), geo_q.georel.clone()) else {
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
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        answering_store::{AnsweringStore, StoredAnswer},
        cache::SubscriptionCache,
        change::SubscriptionChange,
        compiled::CompiledSubscription,
        error::SubscriptionError,
        geo_filter::SubscriptionGeoFilter,
        notifying_since::NotifyingSince,
    };
    use andromeda_context_commit::{change_event::ChangeEvent, store_instant::StoreInstant, subscription_key::SubscriptionKey};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_entity::reads::{
        attribute_change::{AttributeChange, AttributeChangeKind},
        attribute_instance_key::AttributeInstanceKey,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        notification::ChangeOrigin,
        subscription::{SubscriptionStatus, entity_selector::EntitySelector, standing::SubscriptionStanding},
    };
    use andromeda_ngsi_ld_query::{filter::language_matching::LanguageMatching, projection::compiled::Projection};
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_retry::immediate_retry_delay::ImmediateRetryDelay;
    use andromeda_tenancy::tenant_ref::TenantRef;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use std::{num::NonZeroU32, sync::Arc};
    use url::Url;

    fn live_target() -> ContextTarget {
        ContextTarget::Live
    }

    fn make_compiled_sub(id: &str, key: i64, type_: &str, triggers: Vec<&str>) -> CompiledSubscription {
        CompiledSubscription {
            id: format!("urn:ngsi-ld:Subscription:{id}").parse().expect("invalid fixture subscription URI:"),
            key: SubscriptionKey::try_from(key).expect("invalid fixture subscription key:"),
            standing: SubscriptionStanding {
                status: SubscriptionStatus::Active,
                expires_at: None,
            },
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
            notification_attributes: Vec::new(),
            notification_format: None,
            notification_sys_attrs: SysAttrsInclusion::Strip,
            notification_show_changes: false,
            jsonld_context: None,
            notifying_since: NotifyingSince {
                active: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
                selected: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
            },
            compiled_q: None,
            language_matching: LanguageMatching::TagEquality,
            compiled_geo_q: SubscriptionGeoFilter::Unrestricted,
            compiled_scope_q: None,
            dataset_id: None,
            notification_join: None,
            notification_join_level: NonZeroU32::MIN,
            compiled_csf: None,
            local_only: false,
            ngsild_conformance: None,
            lang: None,
            notification_projection: Projection::default(),
        }
    }

    #[test]
    fn len_reflects_cache_size() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
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
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        let target = live_target();
        let subscription = make_compiled_sub("keyed", 7, "Vehicle", vec!["entityCreated"]);
        let id = subscription.id.clone(); // clone: the lookup outlives the value the map takes
        cache.insert(&target, subscription);

        assert_eq!(cache.get(&target, &id).map(|compiled| compiled.key), SubscriptionKey::try_from(7).ok());
    }

    fn upserted(target: &ContextTarget, id: &Identifier) -> SubscriptionChange {
        SubscriptionChange::Upserted {
            target: target.clone(), // clone: the change owns its target
            id: id.clone(),         // clone: the change owns its id
        }
    }

    /// A subscription the store could not be asked about is not a subscription
    /// the store no longer holds: the failure is reported, the cached entry
    /// stays so a caller that retries still finds it, and the subscription is
    /// marked for the reconcile pass.
    #[tokio::test]
    async fn a_refresh_the_store_cannot_answer_reports_the_failure_keeps_the_entry_and_marks_it_dirty() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        let target = live_target();
        let subscription = make_compiled_sub("unreadable", 3, "Vehicle", vec!["entityCreated"]);
        let id = subscription.id.clone(); // clone: the lookup outlives the value the map takes
        cache.insert(&target, subscription);

        let refreshed = cache.refresh(&target, &id, &AnsweringStore::new(StoredAnswer::Unreadable)).await;

        assert!(matches!(refreshed, Err(SubscriptionError::StoreUnavailable)), "{refreshed:?}");
        assert!(cache.get(&target, &id).is_some(), "an unanswered refresh must not drop the cached subscription");
        assert_eq!(cache.dirty_len(), 1);
    }

    /// A request applies a change with one read: the retry belongs to the
    /// reconcile pass, so the request never waits on a failing store.
    #[tokio::test]
    async fn a_change_whose_read_fails_is_read_once_and_left_dirty_with_its_entry() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        let target = live_target();
        let subscription = make_compiled_sub("lost", 6, "Vehicle", vec!["entityCreated"]);
        let id = subscription.id.clone(); // clone: the lookup outlives the value the map takes
        cache.insert(&target, subscription);
        let store = AnsweringStore::new(StoredAnswer::Unreadable);

        let handled = cache.handle_change(&upserted(&target, &id), &store).await;

        assert!(matches!(handled, Err(SubscriptionError::StoreUnavailable)), "{handled:?}");
        assert_eq!(store.reads(), 1);
        assert_eq!(cache.dirty_len(), 1);
        assert!(cache.get(&target, &id).is_some());
    }

    /// The reconcile pass reads a dirty subscription again until the store
    /// answers, then indexes it and clears the mark.
    #[tokio::test]
    async fn a_reconcile_whose_store_recovers_indexes_the_subscription_and_clears_the_dirty_mark() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        let target = live_target();
        let id: Identifier = "urn:ngsi-ld:Subscription:recovering".parse().expect("a valid identifier");
        let store = AnsweringStore::new(StoredAnswer::UnreadableForReads(2));
        cache.handle_change(&upserted(&target, &id), &store).await.expect_err("the first read fails");
        assert!(cache.get(&target, &id).is_none());

        cache.reconcile_dirty(&store, &ImmediateRetryDelay).await;

        assert_eq!(cache.dirty_len(), 0);
        assert_eq!(store.reads(), 3);
        assert!(cache.get(&target, &id).is_some(), "the third read found the subscription, so it is indexed");
    }

    /// A subscription whose every read fails stays dirty after the pass spent
    /// its attempts, for the next pass to try again.
    #[tokio::test]
    async fn a_dirty_subscription_whose_every_read_fails_stays_dirty_after_the_attempts() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        let target = live_target();
        let id: Identifier = "urn:ngsi-ld:Subscription:unreachable".parse().expect("a valid identifier");
        let store = AnsweringStore::new(StoredAnswer::Unreadable);
        cache.handle_change(&upserted(&target, &id), &store).await.expect_err("the read fails");

        cache.reconcile_dirty(&store, &ImmediateRetryDelay).await;

        assert_eq!(cache.dirty_len(), 1);
        assert_eq!(store.reads(), 1 + super::REFRESH_RETRY.attempts.get());
    }

    /// A row that cannot be decoded is not mended by reading it again, so no
    /// pass reads it.
    #[tokio::test]
    async fn a_change_whose_row_cannot_be_decoded_is_not_marked_for_reconcile() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        let target = live_target();
        let id: Identifier = "urn:ngsi-ld:Subscription:undecodable".parse().expect("a valid identifier");
        let store = AnsweringStore::new(StoredAnswer::UndecodableRow);
        let handled = cache.handle_change(&upserted(&target, &id), &store).await;
        assert!(matches!(handled, Err(SubscriptionError::Representation(_))), "{handled:?}");

        cache.reconcile_dirty(&store, &ImmediateRetryDelay).await;

        assert_eq!(cache.dirty_len(), 0);
        assert_eq!(store.reads(), 1);
    }

    /// The reconcile read was sent before the change committed, and the
    /// change's own read could not reach the store: the older answer must not
    /// index the subscription, nor clear the mark the change needs.
    #[tokio::test]
    async fn a_read_older_than_a_failed_change_neither_indexes_nor_clears_the_dirty_mark() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        let target = live_target();
        let stale = make_compiled_sub("raced", 10, "Vehicle", vec!["entityCreated"]);
        let id = stale.id.clone(); // clone: the lookup outlives the value the map takes
        let reconcile = cache.refreshes.begin(&target, &id);

        cache
            .handle_change(&upserted(&target, &id), &AnsweringStore::new(StoredAnswer::Unreadable))
            .await
            .expect_err("the change's read fails");
        reconcile.answered(|| cache.insert(&target, stale));

        assert!(cache.get(&target, &id).is_none());
        assert_eq!(cache.dirty_len(), 1);
    }

    /// A slow read answers after a newer one found the subscription gone: the
    /// older version it read is not indexed again.
    #[tokio::test]
    async fn a_read_older_than_an_answered_one_does_not_overwrite_it() -> Result<(), SubscriptionError> {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        let target = live_target();
        let stale = make_compiled_sub("overtaken", 11, "Vehicle", vec!["entityCreated"]);
        let id = stale.id.clone(); // clone: the lookup outlives the value the map takes
        let slow = cache.refreshes.begin(&target, &id);

        cache.refresh(&target, &id, &AnsweringStore::new(StoredAnswer::Absent)).await?;
        slow.answered(|| cache.insert(&target, stale));

        assert!(cache.get(&target, &id).is_none());
        Ok(())
    }

    /// A refresh begun before the subscription was deleted answers with the
    /// version it read; the deletion is newer, so the subscription stays gone.
    #[tokio::test]
    async fn a_read_begun_before_a_deletion_does_not_index_the_subscription_again() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        let target = live_target();
        let stale = make_compiled_sub("deleted-meanwhile", 12, "Vehicle", vec!["entityCreated"]);
        let id = stale.id.clone(); // clone: the lookup outlives the value the map takes
        let slow = cache.refreshes.begin(&target, &id);

        cache
            .handle_change(
                &SubscriptionChange::Deleted {
                    target: target.clone(), // clone: the change owns its target
                    id: id.clone(),         // clone: the change owns its id
                },
                &AnsweringStore::new(StoredAnswer::Absent),
            )
            .await
            .expect("a delete reads nothing");
        slow.answered(|| cache.insert(&target, stale));

        assert!(cache.get(&target, &id).is_none());
    }

    /// A subscription deleted while it was dirty has nothing left to read.
    #[tokio::test]
    async fn a_delete_of_a_dirty_subscription_clears_the_dirty_mark() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        let target = live_target();
        let id: Identifier = "urn:ngsi-ld:Subscription:deleted".parse().expect("a valid identifier");
        let store = AnsweringStore::new(StoredAnswer::Unreadable);
        cache.handle_change(&upserted(&target, &id), &store).await.expect_err("the read fails");
        assert_eq!(cache.dirty_len(), 1);

        cache
            .handle_change(
                &SubscriptionChange::Deleted {
                    target: target.clone(), // clone: the change owns its target
                    id: id.clone(),         // clone: the change owns its id
                },
                &store,
            )
            .await
            .expect("a delete reads nothing");

        assert_eq!(cache.dirty_len(), 0);
        assert_eq!(store.reads(), 1);
    }

    /// A subscription the store no longer holds is dropped from the cache.
    #[tokio::test]
    async fn a_refresh_that_finds_the_subscription_gone_drops_it() -> Result<(), SubscriptionError> {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        let target = live_target();
        let subscription = make_compiled_sub("gone", 4, "Vehicle", vec!["entityCreated"]);
        let id = subscription.id.clone(); // clone: the lookup outlives the value the map takes
        cache.insert(&target, subscription);

        cache.refresh(&target, &id, &AnsweringStore::new(StoredAnswer::Absent)).await?;

        assert!(cache.get(&target, &id).is_none());
        Ok(())
    }

    fn at(unix_seconds: i64) -> StoreInstant {
        StoreInstant::new(DateTime::from_timestamp(unix_seconds, 0).expect("a fixture timestamp in range"))
    }

    /// The one Vehicle subscription every version below is of, watching
    /// `attribute` as it does since `selected`.
    fn watching(attribute: &str, selected: i64) -> CompiledSubscription {
        let mut subscription = make_compiled_sub("churn", 9, "Vehicle", vec!["attributeUpdated"]);
        subscription.watched_attrs = Some([attribute.parse().expect("a valid attribute name")].into_iter().collect());
        subscription.notifying_since = NotifyingSince {
            active: at(0),
            selected: at(selected),
        };
        subscription
    }

    /// An update of `attribute` on a Vehicle.
    fn update_of(attribute: &str) -> ChangeEvent {
        ChangeEvent::builder()
            .entity_id("urn:ngsi-ld:Vehicle:1".parse().expect("a valid identifier"))
            .entity_types(vec!["Vehicle".parse().expect("a valid type name")])
            .change(EntityChange::new(
                EntityLifecycle::Modified,
                vec![AttributeChange {
                    key: AttributeInstanceKey {
                        attr: attribute.parse().expect("a valid attribute name"),
                        dataset_id: None,
                    },
                    kind: AttributeChangeKind::Updated,
                }],
            ))
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .build()
    }

    /// The `watchedAttributes` of each version a change committed at
    /// `committed_at` was matched by.
    fn matched_watching(cache: &SubscriptionCache, attribute: &str, committed_at: i64) -> Vec<Vec<String>> {
        cache
            .find_matching_committed_at(&live_target(), &update_of(attribute), at(committed_at))
            .matched
            .iter()
            .map(|version| version.watched_attrs.iter().flatten().map(ToString::to_string).collect())
            .collect()
    }

    /// `seq` is watched from 1 000, `other` from 2 000.
    fn reselected() -> SubscriptionCache {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        cache.insert(&live_target(), watching("seq", 1_000));
        cache.insert(&live_target(), watching("other", 2_000));
        cache
    }

    /// Clause 5.8.6: a change of `seq` committed at 1 500, while `seq` was
    /// watched, is owed however late it is evaluated.
    #[test]
    fn a_change_committed_under_a_superseded_selection_is_matched_by_it() {
        let cache = reselected();
        assert_eq!(matched_watching(&cache, "seq", 1_500), vec![vec!["seq".to_owned()]]);
        assert_eq!(matched_watching(&cache, "other", 1_500), Vec::<Vec<String>>::new());
    }

    #[test]
    fn a_change_committed_after_the_update_is_matched_only_by_the_selection_in_force() {
        let cache = reselected();
        assert_eq!(matched_watching(&cache, "seq", 2_500), Vec::<Vec<String>>::new());
        assert_eq!(matched_watching(&cache, "other", 2_500), vec![vec!["other".to_owned()]]);
        assert_eq!(
            matched_watching(&cache, "other", 2_000),
            vec![vec!["other".to_owned()]],
            "the update's own instant is the new selection's"
        );
    }

    /// The versions' intervals are disjoint, so a change both select is
    /// matched once, by the one in force at its commit.
    #[test]
    fn a_change_two_versions_select_is_matched_once_by_the_one_that_stood_at_its_commit() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        cache.insert(&live_target(), watching("seq", 1_000));
        let mut requalified = watching("seq", 2_000);
        requalified.throttling_secs = Some(5.0);
        cache.insert(&live_target(), requalified);
        for committed_at in [1_500, 2_500] {
            let matched = cache.find_matching_committed_at(&live_target(), &update_of("seq"), at(committed_at)).matched;
            assert_eq!(matched.len(), 1, "committed at {committed_at}");
            let in_force = cache.is_in_force(&live_target(), &matched[0]);
            assert_eq!(in_force, committed_at >= 2_000, "committed at {committed_at}");
        }
    }

    #[test]
    fn an_update_that_keeps_the_selection_supersedes_nothing() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        cache.insert(&live_target(), watching("seq", 1_000));
        cache.insert(&live_target(), watching("seq", 1_000));
        assert_eq!(cache.superseded_len(&live_target()), 0);
    }

    /// Table 5.2.12-1 `timeInterval`: a periodic selection was owed no change.
    #[test]
    fn a_periodic_selection_is_not_kept() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        let mut periodic = watching("seq", 1_000);
        periodic.time_interval_secs = Some(10.0);
        cache.insert(&live_target(), periodic);
        cache.insert(&live_target(), watching("other", 2_000));
        assert_eq!(cache.superseded_len(&live_target()), 0);
    }

    /// Clause 5.8.6 sends nothing to a Subscription no longer active, so a
    /// deleted, paused or expired one drops what it superseded.
    #[test]
    fn removing_a_subscription_drops_the_selections_it_superseded() {
        let cache = reselected();
        let id = watching("seq", 0).id;
        cache.remove(&live_target(), &id);
        assert_eq!(cache.superseded_len(&live_target()), 0);
        assert_eq!(matched_watching(&cache, "seq", 1_500), Vec::<Vec<String>>::new());
    }

    /// The selection in force decides whether the Subscription is still
    /// notified of changes; one turned periodic since is not.
    #[test]
    fn a_superseded_selection_is_not_matched_once_the_subscription_is_no_longer_notified_of_changes() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        cache.insert(&live_target(), watching("seq", 1_000));
        let mut periodic = watching("other", 2_000);
        periodic.time_interval_secs = Some(10.0);
        cache.insert(&live_target(), periodic);
        assert_eq!(cache.superseded_len(&live_target()), 1);
        assert_eq!(matched_watching(&cache, "seq", 1_500), Vec::<Vec<String>>::new());
    }

    #[test]
    fn a_superseded_selection_is_kept_while_a_change_committed_before_its_end_is_held() {
        let cache = reselected();
        cache.release_superseded(Some(at(1_999)));
        assert_eq!(cache.superseded_len(&live_target()), 1);
    }

    #[test]
    fn a_superseded_selection_is_released_once_no_change_committed_before_its_end_is_held() {
        for floor in [Some(at(2_000)), Some(at(2_500)), None] {
            let cache = reselected();
            cache.release_superseded(floor);
            assert_eq!(cache.superseded_len(&live_target()), 0, "{floor:?}");
            assert_eq!(matched_watching(&cache, "seq", 1_500), Vec::<Vec<String>>::new(), "{floor:?}");
        }
    }

    #[test]
    fn only_the_cached_version_is_in_force() {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        let superseded = Arc::new(watching("seq", 1_000));
        cache.indices.write().entry(live_target()).or_default().insert(Arc::clone(&superseded));
        cache.insert(&live_target(), watching("other", 2_000));
        let in_force = cache.get(&live_target(), &superseded.id).expect("the subscription is cached");
        assert!(cache.is_in_force(&live_target(), &in_force));
        assert!(!cache.is_in_force(&live_target(), &superseded));
    }
}
