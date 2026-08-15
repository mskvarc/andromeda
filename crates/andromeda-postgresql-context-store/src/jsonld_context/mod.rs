//! Persistence layer for JSON-LD context documents.
//!
//! - [`repo`] — the shared repository handle.
//! - [`rows`] — `sqlx::FromRow` structs.
//! - [`mapper`] — row → domain parsing.
//! - [`registry_store`] — hosted-context lifecycle persistence.
//! - [`context_cache`] — JSON-LD resolver cache persistence.
//! - [`hit_store`] — durable resolver-hit accounting.

/// JSON-LD resolver cache persistence.
pub mod context_cache;
/// Durable resolver-hit accounting.
pub mod hit_store;
/// Row-to-domain conversions for JSON-LD context documents.
pub mod mapper;
/// Hosted-context lifecycle persistence.
pub mod registry_store;
/// The PostgreSQL JSON-LD context store.
pub mod repo;
/// Row structures for JSON-LD context documents.
pub mod rows;
/// SeaQuery statement builders for the `jsonld_contexts` relation.
pub mod statement;
