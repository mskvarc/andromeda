//! Compiled context-source-subscription cache.

use crate::{compiled::CompiledCsrSubscription, matching::matches_csource_subscription};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use dashmap::DashMap;
use serde_json::Value;
use std::sync::Arc;

/// Thread-safe compiled context-source-subscription cache.
#[derive(Debug, Default)]
pub struct CsourceSubscriptionCache {
    inner: DashMap<Uri, Arc<CompiledCsrSubscription>>,
}

impl CsourceSubscriptionCache {
    /// Construct an empty context-source-subscription cache.
    pub fn new() -> CsourceSubscriptionCache {
        CsourceSubscriptionCache { inner: DashMap::new() }
    }

    /// Remove every compiled subscription.
    pub fn clear(&self) {
        self.inner.clear();
    }

    /// Insert or replace one compiled subscription.
    pub fn insert(&self, id: Uri, subscription: Arc<CompiledCsrSubscription>) {
        self.inner.insert(id, subscription);
    }

    /// Remove one compiled subscription.
    pub fn remove(&self, id: &Uri) {
        self.inner.remove(id);
    }

    /// Return the number of compiled subscriptions.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether the cache contains no compiled subscriptions.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Find compiled subscriptions matching one context-source registration representation.
    pub fn find_matching(&self, registration: &Value) -> Vec<Arc<CompiledCsrSubscription>> {
        self.inner
            .iter()
            .filter_map(|entry| matches_csource_subscription(entry.value(), registration).then(|| Arc::clone(entry.value())))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::csource_cache::CsourceSubscriptionCache;

    #[test]
    fn a_new_csource_subscription_cache_is_empty() {
        let cache = CsourceSubscriptionCache::new();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }
}
