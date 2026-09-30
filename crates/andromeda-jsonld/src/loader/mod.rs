//! Resolving a context URL to a context document.
//!
//! [`http::HttpContextLoader`] is what the JSON-LD processor calls whenever a
//! document names a context it has not seen. It resolves in four steps — the
//! in-process moka cache, the [`crate::context_registry::ContextRegistry`] of
//! embedded and preloaded documents, the
//! [`crate::context_cache::ContextCache`] the broker attaches, and finally an
//! HTTP fetch when one is permitted.
//!
//! The steps are split by what they own rather than kept in one file: what a
//! response grants a cache ([`cache_lifetime`]), how a document is downloaded
//! and validated ([`remote_document`]), against which URL its relative
//! context references resolve (`relative_context_references`), how the
//! durable tier is read and
//! written ([`durable_cache`]), what an in-process entry is and how long it
//! lives ([`loaded_context`]), what a context owes when its durable write
//! failed ([`pending_persistence`]), how a reference to one of the tenant's
//! own stored `@context`s is resolved (`own_context`), and which
//! resolutions count as clause 5.13.3.5 hits (`access_accounting`).

/// Counting the accesses clause 5.13.3.5 reports as `numberOfHits`.
mod access_accounting;
/// Finding the JSON-LD alternate a non-JSON response names.
pub(crate) mod alternate_link;
/// The lifetime a response grants a downloaded `@context` (clause 6.3.16).
pub mod cache_lifetime;
/// The lifetimes the resolver assigns where no response states one.
pub(crate) mod context_lifetimes;
/// The durable-cache steps of context resolution.
pub mod durable_cache;
/// Failures reported while resolving a context URL.
pub mod error;
/// The four-step resolution pipeline.
pub mod http;
/// One resolved `@context` as the in-process cache holds it.
pub mod loaded_context;
/// The deployment settings one loader is built with.
pub(crate) mod loader_settings;
/// The settings unit tests build an offline loader with.
#[cfg(test)]
pub(crate) mod offline_settings;
/// Resolving a reference to one of the tenant's own stored `@context`s.
pub(crate) mod own_context;
/// What a context keeps while its durable write is still owed.
pub mod pending_persistence;
/// Resolving a downloaded document's relative context references against the
/// URL it was finally read from.
pub(crate) mod relative_context_references;
/// Downloading and parsing one remote `@context` document.
pub mod remote_document;
/// A context host for loader tests, on the workspace's mock server.
#[cfg(test)]
pub(crate) mod test_origin;
