//! Persistence layer for the notification outbox.
//!
//! - [`repo`] — SQL execution only.
//! - [`rows`] — `sqlx::FromRow` structs (crate-visible).
//! - [`mapper`] — row → domain parsing.

pub mod entry;
pub mod mapper;
pub mod repo;
pub(crate) mod rows;
