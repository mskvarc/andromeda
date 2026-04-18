//! Persistence layer for JSON-LD context documents.
//!
//! - [`repo`] — SQL execution only.
//! - [`rows`] — `sqlx::FromRow` structs.
//! - [`params`] — repository input parameters.
//! - [`mapper`] — row → domain parsing.

pub mod mapper;
pub mod params;
pub mod repo;
pub(crate) mod rows;
