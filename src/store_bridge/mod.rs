//! Remaining binary-owned bindings for policies not yet extracted.
//!
//! Entity and side-effect ports are implemented directly by the PostgreSQL
//! context store; only snapshot and outbox bridges remain here until their
//! policy crates are extracted.

mod outbox;
mod snapshot;
