//! Axum extractors for HTTP concerns shared across NGSI-LD API handlers.
//!
//! Each extractor composes typed header parsing (from [`crate::http::header`]) and/or
//! query parameter parsing (from [`crate::http::parameter`]) into a single
//! [`FromRequestParts`](axum::extract::FromRequestParts) implementation.

pub mod accept;
pub mod context;
pub mod error;
pub mod language;
pub mod prefer;
pub mod query;
