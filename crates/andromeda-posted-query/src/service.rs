//! Holding and resolving posted queries against their store.

use crate::{
    error::PostedQueryError,
    posted_query::{PostedQuery, PostedQueryKind},
    retention::PostedQueryRetention,
    store::PostedQueryStore,
};
use andromeda_ngsi_ld_request::pagination::page_query::PageQuery;
use chrono::{TimeDelta, Utc};
use std::sync::Arc;

/// Holds the queries clients send by POST for the page links of their result,
/// independently of the storage backend.
pub struct PostedQueryService {
    /// The selected posted-query storage implementation.
    store: Arc<dyn PostedQueryStore>,
    /// How long a query is held, and how many one tenant keeps.
    retention: PostedQueryRetention,
}

impl PostedQueryService {
    /// Create a posted-query service backed by the selected store.
    pub const fn new(store: Arc<dyn PostedQueryStore>, retention: PostedQueryRetention) -> PostedQueryService {
        PostedQueryService { store, retention }
    }

    /// Hold `query` for one lifetime from now, so the page links of its
    /// result can name it.
    ///
    /// A query already held under the same key has its lifetime counted again
    /// from now. The tenant's other queries that expire soonest are then
    /// deleted until it is within its quota.
    ///
    /// # Errors
    ///
    /// [`PostedQueryError::LifetimeOutOfRange`] when the lifetime counted from
    /// now passes the representable time range, and the store's error when it
    /// cannot hold the query.
    pub async fn hold(&self, query: &PostedQuery) -> Result<(), PostedQueryError> {
        let lifetime = TimeDelta::from_std(self.retention.lifetime).map_err(|_| PostedQueryError::LifetimeOutOfRange)?;
        let expires_at = Utc::now().checked_add_signed(lifetime).ok_or(PostedQueryError::LifetimeOutOfRange)?;
        self.store.hold(query, expires_at, self.retention.quota).await
    }

    /// The live query `key` names among those posted to `kind`.
    ///
    /// # Errors
    ///
    /// [`PostedQueryError::NotFound`] when no live query posted to `kind`
    /// carries the key, and the store's error when it cannot be read.
    pub async fn resolve(&self, key: &PageQuery, kind: PostedQueryKind) -> Result<PostedQuery, PostedQueryError> {
        self.store.get(key, kind).await?.ok_or_else(|| PostedQueryError::NotFound {
            key: key.clone(), // clone: the error must own the key after this call returns.
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::PostedQueryError,
        posted_query::{PostedQuery, PostedQueryKind},
        retention::{PostedQueryQuota, PostedQueryRetention},
        service::PostedQueryService,
        store::{PostedQueryStore, PostedQueryStoreFuture},
    };
    use andromeda_jsonld::request_context::RequestContext;
    use andromeda_ngsi_ld_request::pagination::page_query::PageQuery;
    use chrono::{DateTime, TimeDelta, Utc};
    use pretty_assertions::assert_eq;
    use std::{
        num::NonZeroUsize,
        sync::{Arc, Mutex},
        time::Duration,
    };

    /// One `hold` the store was asked for.
    #[derive(Debug)]
    struct HoldCall {
        key: PageQuery,
        expires_at: DateTime<Utc>,
        quota: PostedQueryQuota,
    }

    /// A store holding at most one query, recording every `hold`.
    #[derive(Default)]
    struct RecordingStore {
        held: Mutex<Option<PostedQuery>>,
        holds: Mutex<Vec<HoldCall>>,
    }

    impl PostedQueryStore for RecordingStore {
        fn hold<'a>(&'a self, query: &'a PostedQuery, expires_at: DateTime<Utc>, quota: PostedQueryQuota) -> PostedQueryStoreFuture<'a, ()> {
            Box::pin(async move {
                self.holds.lock().expect("unpoisoned").push(HoldCall {
                    key: query.key().clone(),
                    expires_at,
                    quota,
                });
                *self.held.lock().expect("unpoisoned") = Some(query.clone());
                Ok(())
            })
        }

        fn get<'a>(&'a self, key: &'a PageQuery, kind: PostedQueryKind) -> PostedQueryStoreFuture<'a, Option<PostedQuery>> {
            Box::pin(async move {
                Ok(self
                    .held
                    .lock()
                    .expect("unpoisoned")
                    .clone()
                    .filter(|held| held.key() == key && held.kind() == kind))
            })
        }

        fn delete_expired(&self, _cutoff: DateTime<Utc>) -> PostedQueryStoreFuture<'_, u64> {
            Box::pin(async { Ok(0) })
        }
    }

    fn retention() -> PostedQueryRetention {
        PostedQueryRetention {
            lifetime: Duration::from_secs(3_600),
            quota: PostedQueryQuota {
                max_count: NonZeroUsize::new(3),
                max_bytes: Some(1_024),
            },
        }
    }

    fn query(kind: PostedQueryKind) -> PostedQuery {
        let body = serde_json::from_str(r#"{"type":"Query","entities":[{"type":"Vehicle"}]}"#).expect("a clause 5.2.23 Query");
        PostedQuery::new(kind, body, RequestContext::Default).expect("serialises")
    }

    #[tokio::test]
    async fn a_query_is_held_one_lifetime_from_now_within_the_quota() {
        let store = Arc::new(RecordingStore::default());
        let service = PostedQueryService::new(Arc::clone(&store) as Arc<dyn PostedQueryStore>, retention());
        let posted = query(PostedQueryKind::Entities);
        let before = Utc::now();
        service.hold(&posted).await.expect("held");
        let after = Utc::now();
        let holds = store.holds.lock().expect("unpoisoned");
        let [call] = holds.as_slice() else {
            panic!("one hold, not {holds:?}");
        };
        assert_eq!(&call.key, posted.key());
        assert_eq!(call.quota, retention().quota);
        let hour = TimeDelta::hours(1);
        assert!(before + hour <= call.expires_at && call.expires_at <= after + hour, "{call:?}");
    }

    #[tokio::test]
    async fn a_held_query_is_resolved_by_its_key_and_api() {
        let store = Arc::new(RecordingStore::default());
        let service = PostedQueryService::new(Arc::clone(&store) as Arc<dyn PostedQueryStore>, retention());
        let posted = query(PostedQueryKind::Entities);
        service.hold(&posted).await.expect("held");
        assert_eq!(service.resolve(posted.key(), PostedQueryKind::Entities).await.expect("resolved"), posted);
    }

    #[tokio::test]
    async fn a_key_no_live_query_of_the_api_carries_is_not_found() {
        let store = Arc::new(RecordingStore::default());
        let service = PostedQueryService::new(Arc::clone(&store) as Arc<dyn PostedQueryStore>, retention());
        let posted = query(PostedQueryKind::Entities);
        service.hold(&posted).await.expect("held");
        let refused = service.resolve(posted.key(), PostedQueryKind::Temporal).await;
        assert!(
            matches!(refused, Err(PostedQueryError::NotFound { ref key }) if key == posted.key()),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn a_lifetime_past_the_representable_range_is_refused() {
        let service = PostedQueryService::new(
            Arc::new(RecordingStore::default()),
            PostedQueryRetention {
                lifetime: Duration::MAX,
                ..retention()
            },
        );
        let refused = service.hold(&query(PostedQueryKind::Temporal)).await;
        assert!(matches!(refused, Err(PostedQueryError::LifetimeOutOfRange)), "{refused:?}");
    }
}
