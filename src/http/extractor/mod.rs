//! Axum extractors for HTTP concerns shared across NGSI-LD API handlers.
//!
//! Each extractor composes typed header parsing (from
//! [`andromeda_ngsi_ld_http_headers`]) and/or model-level parsing into a single
//! [`FromRequestParts`](axum::extract::FromRequestParts) implementation.

pub mod accept;
pub mod batch;
pub mod context;
pub mod entity_query;
pub mod error;
pub mod json_body;
pub mod jsonld;
pub mod language;
pub mod prefer;
pub mod protocol;
pub mod query;
pub mod snapshot;
pub mod snapshot_purge_q;
pub mod tenant;
pub mod via;
