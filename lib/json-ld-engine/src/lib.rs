//! Standalone JSON-LD processing engine for NGSI-LD.
//!
//! This crate provides a self-contained JSON-LD engine that can expand and
//! compact documents according to the NGSI-LD specification.  It is decoupled
//! from any particular persistence layer — external caches (e.g. database-backed)
//! can be plugged in via the [`ContextCache`] trait.
//!
//! # Quick start
//!
//! ```ignore
//! use json_ld_engine::{JsonLdEngine, ContextSpec};
//!
//! let engine = JsonLdEngine::new()?;
//! let expanded = engine.expand(document, &ContextSpec::none()).await?;
//! ```

mod context;
pub mod core_context;
mod engine;
mod error;
mod expandable;
mod loader;

// Re-export public API.
pub use context::{ContextMerge, ContextSpec, CoreContextPolicy, apply_context, default_core_context_url, resolve_context_value, set_default_core_context_url};
pub use engine::{CompactOptions, JsonLdEngine, JsonLdEngineBuilder};
pub use error::Error;
pub use expandable::JsonLdExpandable;
pub use loader::{ContextCache, ContextRegistry, HttpContextLoader};
