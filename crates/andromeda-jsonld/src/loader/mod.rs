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
//! and validated ([`remote_document`]), how the durable tier is read and
//! written ([`durable_cache`]), what an in-process entry is and how long it
//! lives ([`loaded_context`]), and what a context owes when its durable write
//! failed ([`pending_persistence`]).

/// The lifetime a response grants a downloaded `@context` (clause 6.3.16).
pub mod cache_lifetime;
/// The durable-cache steps of context resolution.
pub mod durable_cache;
/// Failures reported while resolving a context URL.
pub mod error;
/// The four-step resolution pipeline.
pub mod http;
/// One resolved `@context` as the in-process cache holds it.
pub mod loaded_context;
/// What a context keeps while its durable write is still owed.
pub mod pending_persistence;
/// Downloading and parsing one remote `@context` document.
pub mod remote_document;
