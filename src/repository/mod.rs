//! Persistence layer for the NGSI-LD context broker.
//!
//! The bulk of the repository implementations now live in
//! `andromeda-postgresql-context-store` and `andromeda-postgresql-temporal-store`.
//! This module retains only the composition root that wires them together.

/// Committing atomic writes against PostgreSQL.
pub mod atomic_write;
