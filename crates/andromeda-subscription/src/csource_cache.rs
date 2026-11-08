//! Compiled context-source-subscription cache.

use crate::compiled::CompiledCsrSubscription;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use dashmap::DashMap;
use std::sync::Arc;

/// Thread-safe compiled context-source-subscription cache.
#[derive(Debug, Default)]
pub struct CsourceSubscriptionCache {
    /// The compiled subscriptions, by subscription id.
    inner: DashMap<Identifier, Arc<CompiledCsrSubscription>>,
}

impl CsourceSubscriptionCache {
    /// Construct an empty context-source-subscription cache.
    #[must_use]
    pub fn new() -> CsourceSubscriptionCache {
        CsourceSubscriptionCache { inner: DashMap::new() }
    }

    /// Remove every compiled subscription.
    pub fn clear(&self) {
        self.inner.clear();
    }

    /// Insert or replace one compiled subscription.
    pub fn insert(&self, id: Identifier, subscription: Arc<CompiledCsrSubscription>) {
        self.inner.insert(id, subscription);
    }

    /// Remove one compiled subscription.
    pub fn remove(&self, id: &Identifier) {
        self.inner.remove(id);
    }

    /// Return the number of compiled subscriptions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether the cache contains no compiled subscriptions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Every cached subscription, in no particular order.
    #[must_use]
    pub fn subscriptions(&self) -> Vec<Arc<CompiledCsrSubscription>> {
        self.inner.iter().map(|entry| Arc::clone(entry.value())).collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::{compiled::CompiledCsrSubscription, csource_cache::CsourceSubscriptionCache, geo_filter::SubscriptionGeoFilter};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::subscription::{SubscriptionStatus, standing::SubscriptionStanding};
    use pretty_assertions::assert_eq;
    use std::sync::Arc;
    use url::Url;

    fn subscription(id: &Identifier) -> Arc<CompiledCsrSubscription> {
        Arc::new(CompiledCsrSubscription {
            id: id.clone(),
            standing: SubscriptionStanding {
                status: SubscriptionStatus::Active,
                expires_at: None,
            },
            endpoint_uri: Url::parse("http://example.com/notify").expect("valid URL"),
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_notifier_info: None,
            endpoint_cooldown: None,
            endpoint_timeout: None,
            selectors: Vec::new(),
            matching_attrs: Vec::new(),
            temporal_q: None,
            geo_q: SubscriptionGeoFilter::Unrestricted,
            scope_q: None,
            throttling_secs: None,
            time_interval_secs: None,
            jsonld_context: None,
            compiled_csf: None,
        })
    }

    #[test]
    fn a_new_csource_subscription_cache_is_empty() {
        let cache = CsourceSubscriptionCache::new();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn a_cached_subscription_is_listed_until_removed() {
        let cache = CsourceSubscriptionCache::new();
        let id: Identifier = "urn:ngsi-ld:Subscription:csource".parse().expect("valid URI");
        cache.insert(id.clone(), subscription(&id));

        let listed: Vec<Identifier> = cache.subscriptions().iter().map(|subscription| subscription.id.clone()).collect();
        assert_eq!(listed, vec![id.clone()]);

        cache.remove(&id);
        assert!(cache.subscriptions().is_empty());
    }
}
