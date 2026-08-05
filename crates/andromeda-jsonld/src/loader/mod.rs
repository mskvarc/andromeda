//! Resolving a context URL to a context document.
//!
//! [`http::HttpContextLoader`] is what the JSON-LD processor calls whenever a
//! document names a context it has not seen. It resolves in four steps — the
//! in-process moka cache, the [`crate::context_registry::ContextRegistry`] of
//! embedded and preloaded documents, the
//! [`crate::context_cache::ContextCache`] the broker attaches, and finally an
//! HTTP fetch when one is permitted — and every document that arrives from
//! outside the registry passes through [`url_normalization`] first.

pub mod error;
pub mod http;
pub mod url_normalization;
