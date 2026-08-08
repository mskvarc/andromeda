//! Persistence layer for JSON-LD context documents.
//!
//! - [`repo`] — SQL execution only.
//! - [`rows`] — `sqlx::FromRow` structs.
//! - [`params`] — repository input parameters.
//! - [`mapper`] — row → domain parsing.

/// Row-to-domain conversions for JSON-LD context documents.
pub mod mapper;
/// Repository input parameters for JSON-LD context operations.
pub mod params;
/// The PostgreSQL JSON-LD context store.
pub mod repo;
/// Row structures for JSON-LD context documents.
pub mod rows;
