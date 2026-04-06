//! Axum extractors for HTTP concerns shared across NGSI-LD API handlers.
//!
//! Each extractor composes typed header parsing (from [`crate::http::header`]) and/or
//! model-level parsing (from [`crate::http::model`]) into a single
//! [`FromRequestParts`](axum::extract::FromRequestParts) implementation.

pub mod accept;
pub mod context;
pub mod error;
pub mod language;
pub mod prefer;
pub mod query;
pub mod snapshot;
pub mod tenant;
