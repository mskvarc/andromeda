//! A Subscription as the store hands it back, with the identity it is stored under.

use andromeda_context_commit::subscription_key::SubscriptionKey;
use andromeda_ngsi_ld_model::subscription::Subscription;

/// One stored Subscription together with its surrogate key.
///
/// The resource itself is the spec-facing value and carries only what clause
/// 5.2.12 defines; the key is store-assigned identity that never appears in it.
/// Reads that feed the matching caches return the pair, because a match has to
/// enqueue a delivery against the key while every rendered notification and
/// every lifecycle change keeps naming the Subscription by its URI.
#[derive(Debug)]
pub struct StoredSubscription {
    /// The identity the store assigned this Subscription.
    pub key: SubscriptionKey,
    /// The Subscription resource as stored.
    pub subscription: Subscription,
}
