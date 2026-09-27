//! REG area — Context Source Registration lifecycle and data model
//! (ETSI GS CIM 009 v1.9.1, clauses 5.2.8–5.2.11, 5.2.34, 5.2.40, 5.9.1–5.9.4).
//!
//! Registry-API-local validation and lifecycle run on `T-LOCAL` (broker A
//! alone); the intrinsically multi-instance rows (`T-TENANT`, `T-MOCK`) say so.

mod csr_create;
mod csr_delete;
mod csr_health_fields;
mod csr_lifecycle_expiry;
mod csr_mode_conflict;
mod csr_source_identity;
mod csr_temporal_discovery;
mod csr_update;
mod csr_validation;

/// A syntactically valid peer endpoint for a baseline registration whose other
/// fields are under test. Registration validation is local, so this endpoint is
/// never actually contacted.
pub const PEER_ENDPOINT: &str = "http://broker-b:1056/ngsi-ld/v1";
