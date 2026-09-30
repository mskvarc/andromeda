//! Cached forwarded GET responses (clause 6.3.17, `cacheDuration` of clause 5.2.34).
//!
//! A registration that declares `management.cacheDuration` lets the broker
//! reuse a previous response to the same forwarded `GET /entities` or
//! `GET /entities/{id}` until the duration elapses; clause 6.3.17 requires
//! every such reuse to be flagged with the 110 warning. The cache key carries
//! the registration's `modifiedAt`, so an updated registration never answers
//! from entries built under its previous definition, and a deleted one is
//! never selected at all.

use crate::outcome::ForwardedResponse;
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use iri_rs::IriBuf;
use std::time::{Duration, Instant};
use url::Url;

/// Whether a forwarded GET targets one of the two resources clause 6.3.17
/// allows caching for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cacheability {
    /// `GET /entities` or `GET /entities/{id}`.
    Cacheable,
    /// Any other forwarded GET.
    Uncacheable,
}

/// What identifies one cached response: the registration as defined at the
/// time, the exact forwarded URL and the representation it was asked in.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct CacheKey {
    /// The registration the response came from.
    registration: Identifier,
    /// The registration's definition version.
    modified_at: DateTime<Utc>,
    /// The forwarded URL, query included.
    url: Url,
    /// The context URL the request named in its one context `Link`.
    context: Option<IriBuf>,
}

impl CacheKey {
    /// The key of one forwarded GET.
    pub(crate) fn new(registration: &ContextSourceRegistration, url: &Url, context: Option<&IriBuf>) -> CacheKey {
        CacheKey {
            registration: registration.id.clone(), // clone: the key owns its parts
            modified_at: registration.lifecycle.modified_at,
            url: url.clone(),          // clone: same
            context: context.cloned(), // clone: same
        }
    }
}

/// One cached response and when it stops being reusable.
#[derive(Debug, Clone)]
struct CachedResponse {
    /// What the source answered with.
    response: ForwardedResponse,
    /// The instant the `cacheDuration` elapses.
    expires_at: Instant,
}

/// The bounded cache of forwarded GET responses.
#[derive(Debug)]
pub(crate) struct ForwardedGetCache {
    /// The live entries.
    entries: DashMap<CacheKey, CachedResponse>,
    /// The most entries held at once; a full cache stops admitting.
    max_entries: usize,
}

impl ForwardedGetCache {
    /// A cache holding at most `max_entries` responses.
    pub(crate) fn new(max_entries: usize) -> ForwardedGetCache {
        ForwardedGetCache {
            entries: DashMap::new(),
            max_entries,
        }
    }

    /// The `cacheDuration` of a registration, when it declares one.
    pub(crate) fn duration_of(registration: &ContextSourceRegistration) -> Option<Duration> {
        registration.access.management.as_ref()?.cache_duration?.to_std()
    }

    /// A still-valid cached response, tagged with the 110 warning clause 6.3.17 requires.
    pub(crate) fn lookup(&self, key: &CacheKey) -> Option<ForwardedResponse> {
        let entry = self.entries.get(key)?;
        if entry.expires_at <= Instant::now() {
            drop(entry);
            self.entries.remove(key);
            return None;
        }
        let mut response = entry.response.clone(); // clone: the cache keeps its copy for the next hit
        response.warnings.push(NgsiLdWarning::ResponseIsStale(format!(
            "cached response of registration {} reused within its cacheDuration",
            key.registration
        )));
        Some(response)
    }

    /// Remember a fresh response for `duration`.
    pub(crate) fn store(&self, key: CacheKey, response: &ForwardedResponse, duration: Duration) {
        if self.entries.len() >= self.max_entries {
            let now = Instant::now();
            self.entries.retain(|_, entry| entry.expires_at > now);
            if self.entries.len() >= self.max_entries {
                return;
            }
        }
        self.entries.insert(
            key,
            CachedResponse {
                response: response.clone(), // clone: the cache keeps its own copy beside the response being returned
                expires_at: Instant::now() + duration,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        cache::{CacheKey, ForwardedGetCache},
        outcome::ForwardedResponse,
        source_continuation::SourceContinuation,
    };
    use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use serde_json::{Value, json};
    use std::time::Duration;
    use url::Url;

    fn response(body: Value) -> ForwardedResponse {
        ForwardedResponse {
            body,
            warnings: Vec::new(),
            entity_map: None,
            continuation: SourceContinuation::Complete,
            partial_range: None,
        }
    }

    fn registration(cache_duration: Option<&str>, modified_at: &str) -> ContextSourceRegistration {
        let mut document = json!({
            "id": "urn:ngsi-ld:csr:1",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": [{"entities": [{"type": "https://example.org/Vehicle"}]}],
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": modified_at
        });
        if let Some(cache_duration) = cache_duration {
            document["management"] = json!({"cacheDuration": cache_duration});
        }
        serde_json::from_value(document).expect("valid registration")
    }

    fn url() -> Url {
        Url::parse("https://peer.example.org/ngsi-ld/v1/entities?type=Vehicle").expect("valid URL")
    }

    #[test]
    fn a_hit_within_the_duration_carries_the_110_warning() {
        let cache = ForwardedGetCache::new(8);
        let registration = registration(Some("PT10S"), "2024-01-01T00:00:00Z");
        let key = CacheKey::new(&registration, &url(), None);
        assert!(cache.lookup(&key).is_none());
        cache.store(key.clone(), &response(json!([{"id": "urn:ngsi-ld:Vehicle:1"}])), Duration::from_secs(10)); // clone: looked up below
        let hit = cache.lookup(&key).expect("hit");
        pretty_assertions::assert_eq!(hit.body, json!([{"id": "urn:ngsi-ld:Vehicle:1"}]));
        assert!(matches!(hit.warnings.as_slice(), [NgsiLdWarning::ResponseIsStale(_)]));
    }

    #[test]
    fn an_expired_entry_is_a_miss_and_is_evicted() {
        let cache = ForwardedGetCache::new(8);
        let registration = registration(Some("PT10S"), "2024-01-01T00:00:00Z");
        let key = CacheKey::new(&registration, &url(), None);
        cache.store(key.clone(), &response(json!([])), Duration::ZERO); // clone: looked up below
        assert!(cache.lookup(&key).is_none());
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn an_updated_registration_never_answers_from_its_previous_entries() {
        let cache = ForwardedGetCache::new(8);
        let before = registration(Some("PT10S"), "2024-01-01T00:00:00Z");
        let after = registration(Some("PT10S"), "2024-06-01T00:00:00Z");
        cache.store(CacheKey::new(&before, &url(), None), &response(json!([])), Duration::from_secs(10));
        assert!(cache.lookup(&CacheKey::new(&after, &url(), None)).is_none());
    }

    #[test]
    fn a_full_cache_stops_admitting_rather_than_growing() {
        let cache = ForwardedGetCache::new(1);
        let registration = registration(Some("PT10S"), "2024-01-01T00:00:00Z");
        let first = CacheKey::new(&registration, &url(), None);
        let second = CacheKey::new(
            &registration,
            &Url::parse("https://peer.example.org/ngsi-ld/v1/entities?type=Car").expect("valid URL"),
            None,
        );
        cache.store(first, &response(json!([])), Duration::from_secs(10));
        cache.store(second.clone(), &response(json!([])), Duration::from_secs(10)); // clone: looked up below
        assert!(cache.lookup(&second).is_none());
        pretty_assertions::assert_eq!(cache.entries.len(), 1);
    }

    #[test]
    fn the_duration_comes_from_the_registration_management() {
        pretty_assertions::assert_eq!(
            ForwardedGetCache::duration_of(&registration(Some("PT10S"), "2024-01-01T00:00:00Z")),
            Some(Duration::from_secs(10))
        );
        assert!(ForwardedGetCache::duration_of(&registration(None, "2024-01-01T00:00:00Z")).is_none());
    }
}
