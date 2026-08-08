//! Placeholder module for persistence-shaped row structs.
//!
//! The CSR reader is implemented directly against `sqlx::postgres::PgRow`
//! (see [`crate::csource_registration::mapper::row_to_csr`])
//! because the projection derives geometry columns via `ST_AsGeoJSON` casts
//! and does not map 1:1 to a `#[derive(FromRow)]` struct.
