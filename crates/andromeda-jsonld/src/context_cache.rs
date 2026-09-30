//! The durable context cache this crate asks for and does not provide.
//!
//! Declared here rather than beside an implementation because the crate that
//! calls a port owns it: whether contexts are cached in PostgreSQL, in Redis
//! or nowhere at all is the broker's decision, and nothing in JSON-LD
//! processing may depend on which was made.

use crate::loader::cache_lifetime::{CacheLifetime, ResidualLifetime};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use async_trait::async_trait;
use iri_rs::IriBuf;
use serde_json::Value;
use std::fmt::Debug;
use thiserror::Error;

/// A durable context-cache operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ContextCacheError {
    /// A cached context could not be read.
    #[error("failed to read durable context cache: {0}")]
    Read(#[source] PersistenceFailure),
    /// A context could not be persisted.
    #[error("failed to write durable context cache: {0}")]
    Write(#[source] PersistenceFailure),
}

/// One durable cache hit: the stored document and what is left of its lifetime.
///
/// The residual lifetime crosses the port because clause 6.3.16 makes the span
/// the origin server granted authoritative over every tier that holds the
/// document, not only over the row. Without it the in-process cache could only
/// re-bound a hit to the configured heuristic, and a `@context` the origin
/// server said lives thirty seconds would be served for an hour after a restart
/// or an eviction emptied the in-process tier.
#[derive(Debug, Clone)]
pub struct CachedContext {
    /// The stored `@context` document.
    pub payload: Value,
    /// How much of the entry's lifetime is still ahead of it.
    pub residual: ResidualLifetime,
    /// The URL the stored row keys clause 5.13.3.5 hit accounting on.
    ///
    /// A row is reachable by either identity it holds -- its local id or its
    /// serving/source URL (`identity_matches`) -- so a hosted `@context` a
    /// client echoes back as a relative path resolves by its local id while its
    /// `numberOfHits` and the flush both key on the serving URL. Carrying that
    /// canonical URL back lets the resolver attribute the access to the same key
    /// the metadata is read and flushed under, rather than to the spelling the
    /// request happened to use. `None` when the row has no such URL, in which
    /// case the access is attributed to the requested URL.
    pub canonical_url: Option<IriBuf>,
    /// Where the stored document was read from.
    pub source: StoredDocumentSource,
}

/// Where a stored `@context` document came from, which decides the base its
/// own relative references resolve against.
///
/// JSON-LD resolves a relative reference inside a remote context against the
/// document's own URL (JSON-LD 1.1 API section 4.1.2 step 5.2.1), and a remote
/// document's URL is the one it was finally read from after redirects
/// (JSON-LD 1.1 API section 9.4.3, `documentUrl`). A download may have been
/// redirected or sent to a JSON-LD alternate, so that URL is stored with it
/// rather than recomputed from the URL the row is keyed by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredDocumentSource {
    /// A document the broker downloaded: a clause 5.13.1 "Cached" entry.
    Downloaded {
        /// The URL the document was finally read from.
        document_url: IriBuf,
    },
    /// A document the broker stores itself (clause 5.13.1 "Hosted" or
    /// "`ImplicitlyCreated`"), which was never read from anywhere: its
    /// references resolve against the URL it is resolved by.
    Broker,
}

/// A `@context` document downloaded afresh from its source, as a "Cached" row
/// stores it.
///
/// Clause 5.13.5.4 replaces a reloaded entry's document "with the newly
/// downloaded one", and the row keeps the document as downloaded -- not the copy
/// prepared for processing -- together with the URL it was finally read from,
/// exactly as a first download is stored ([`ContextCache::put`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadedContext {
    /// The downloaded document, reduced to its `@context` subtree (clause
    /// 5.13.2.3).
    pub document: Value,
    /// The URL the document was finally read from, after redirects and a
    /// JSON-LD alternate link.
    pub document_url: IriBuf,
}

/// A cache of fetched context documents that outlives the process.
///
/// Consulted after the embedded registry and before any HTTP request, and
/// written once per URL on the first successful remote fetch.
#[async_trait]
pub trait ContextCache: Send + Sync + Debug {
    /// Look up a previously cached context by URL.
    ///
    /// An entry whose lifetime has elapsed is not a hit: clause 5.13.1 forbids
    /// serving a `"Cached"` `@context` past its expiration, so an implementation
    /// reports `None` for a lapsed row and lets resolution reach the source.
    /// A hit carries what is left of that lifetime, so the caller can hold its
    /// own copy to the same instant the row lapses at.
    #[must_use = "context-cache reads must be awaited"]
    async fn get(&self, url: &IriBuf) -> Result<Option<CachedContext>, ContextCacheError>;

    /// Store a context payload under a URL, for `lifetime`.
    ///
    /// `document_url` is the URL the payload was finally read from, after
    /// redirects and a JSON-LD alternate link; implementations must store it and
    /// report it back as [`StoredDocumentSource::Downloaded`], so a later read
    /// resolves the document's relative references against the same base the
    /// first load did.
    ///
    /// `lifetime` is what clause 6.3.16 derived from the response that carried
    /// the document — its `s-maxage`, `max-age` or `Expires`, or the configured
    /// heuristic when it stated none. Implementations must record it, and must
    /// stop reporting the entry from [`ContextCache::get`] once it has elapsed:
    /// clause 5.13.1 requires the entries flagged `"Cached"` to be invalidated
    /// periodically, and a lapsed entry may never be served again.
    ///
    /// Implementations must not touch `numberOfHits` or the last-usage
    /// instant here: clause 5.13.3.5 accounting accumulates in
    /// [`HitCounter`](crate::hit_counter::HitCounter) and is flushed by a
    /// task the broker wires up, precisely so this write stays off the
    /// expansion path.
    #[must_use = "context-cache writes must be awaited"]
    async fn put(&self, url: &IriBuf, document_url: &IriBuf, payload: &Value, lifetime: CacheLifetime) -> Result<(), ContextCacheError>;
}
