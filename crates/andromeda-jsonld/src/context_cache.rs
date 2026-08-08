//! The durable context cache this crate asks for and does not provide.
//!
//! Declared here rather than beside an implementation because the crate that
//! calls a port owns it: whether contexts are cached in PostgreSQL, in Redis
//! or nowhere at all is the broker's decision, and nothing in JSON-LD
//! processing may depend on which was made.

use async_trait::async_trait;
use iri_rs::IriBuf;
use serde_json::Value;
use std::fmt::Debug;
use thiserror::Error;

/// A durable context-cache operation failed.
#[derive(Debug, Clone, Error)]
pub enum ContextCacheError {
    /// A cached context could not be read.
    #[error("failed to read durable context cache: {0}")]
    Read(String),
    /// A context could not be persisted.
    #[error("failed to write durable context cache: {0}")]
    Write(String),
}

/// A cache of fetched context documents that outlives the process.
///
/// Consulted after the embedded registry and before any HTTP request, and
/// written once per URL on the first successful remote fetch.
#[async_trait]
pub trait ContextCache: Send + Sync + Debug {
    /// Look up a previously cached context by URL.
    #[must_use = "context-cache reads must be awaited"]
    async fn get(&self, url: &IriBuf) -> Result<Option<Value>, ContextCacheError>;

    /// Store a context payload for a URL that has none yet.
    ///
    /// Implementations must not touch `numberOfHits` or the last-usage
    /// instant here: clause 5.13.3.5 accounting accumulates in
    /// [`HitCounter`](crate::hit_counter::HitCounter) and is flushed by a
    /// task the broker wires up, precisely so this write stays off the
    /// expansion path.
    #[must_use = "context-cache writes must be awaited"]
    async fn put(&self, url: &IriBuf, payload: &Value) -> Result<(), ContextCacheError>;
}
