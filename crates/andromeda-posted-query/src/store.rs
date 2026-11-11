//! The replaceable storage capability for posted queries.
//!
//! A page link may reach any instance serving the same tenant, after the one
//! that answered the POST restarted, so the store is durable and tenant-scoped
//! rather than a per-process cache.

use crate::{
    error::PostedQueryError,
    posted_query::{PostedQuery, PostedQueryKind},
    retention::PostedQueryQuota,
};
use andromeda_ngsi_ld_request::pagination::page_query::PageQuery;
use chrono::{DateTime, Utc};
use std::{future::Future, pin::Pin};

/// Owned future returned by an object-safe posted-query store implementation.
pub type PostedQueryStoreFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, PostedQueryError>> + Send + 'a>>;

/// Stores posted queries, with expiry decided by the store's own clock so
/// every instance sharing the store agrees on what is live.
pub trait PostedQueryStore: Send + Sync + 'static {
    /// Hold `query` until `expires_at`, moving the expiry of the query already
    /// held under its key, then delete the held queries that expire soonest,
    /// never `query`, until the tenant is within `quota`.
    fn hold<'a>(&'a self, query: &'a PostedQuery, expires_at: DateTime<Utc>, quota: PostedQueryQuota) -> PostedQueryStoreFuture<'a, ()>;

    /// The live query `key` names among those posted to `kind`, treating an
    /// expired query as absent.
    fn get<'a>(&'a self, key: &'a PageQuery, kind: PostedQueryKind) -> PostedQueryStoreFuture<'a, Option<PostedQuery>>;

    /// Remove every posted query whose expiry is at or before `cutoff`,
    /// returning how many.
    fn delete_expired(&self, cutoff: DateTime<Utc>) -> PostedQueryStoreFuture<'_, u64>;
}
