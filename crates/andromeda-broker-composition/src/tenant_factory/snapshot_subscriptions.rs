//! Loading a new tenant runtime's Snapshot-scoped Subscriptions into its cache.
//!
//! A tenant runtime is rebuilt whenever its tenant comes back after eviction,
//! and its subscription cache starts empty. The live target is loaded with the
//! rest of the storage graph; a Subscription created under `NGSILD-Snapshot`
//! lives in its Snapshot's published branch instead, so every published branch
//! is loaded here too. A branch left out stays unmatched until one of its
//! Subscriptions changes, and the entity changes made under that Snapshot meanwhile
//! go unnotified (clause 5.8.6).

use andromeda_context_space::context_target::ContextTarget;
use andromeda_snapshot::store::SnapshotStore;
use andromeda_subscription::{cache::SubscriptionCache, store::SubscriptionStore};

/// Load the active Subscriptions of every Snapshot the tenant holds into
/// `cache`.
///
/// A branch that cannot be listed is logged and skipped, as the live target's
/// load is: the runtime still serves, and each other branch is loaded.
pub(super) async fn load_snapshot_subscriptions(cache: &SubscriptionCache, snapshots: &dyn SnapshotStore, subscriptions: &(impl SubscriptionStore + ?Sized)) {
    let targets = match snapshots.published_targets().await {
        Ok(targets) => targets,
        Err(error) => {
            tracing::error!(%error, "failed to list the snapshots whose subscriptions the cache loads");
            return;
        }
    };
    for target in targets.into_iter().map(ContextTarget::Snapshot) {
        if let Err(error) = cache.reload(&target, subscriptions).await {
            tracing::error!(%error, ?target, "failed to load a snapshot's subscriptions into the cache");
        }
    }
}
