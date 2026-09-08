//! Three-dimensional `GeoJSON` coordinate integration tests (NGSI-LD 4.7.1, RFC 7946 3.1.1).
//!
//! RFC 7946 3.1.1 allows a position to carry exactly two ordinates (longitude,
//! latitude) or three (longitude, latitude, altitude). Andromeda stores and
//! echoes the altitude verbatim while every spatial predicate defined by
//! NGSI-LD 4.10 evaluates on the two-dimensional footprint only.
//!
//! Naming convention: `<area>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod crud;
mod csr;
mod fixtures;
mod georel;
mod ordering;
mod representation;
mod subscription;
mod temporal;
mod validation;

#[path = "../support/notification_adapter.rs"]
pub mod notification_adapter;
