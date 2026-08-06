//! Scope filtering: the `scopeQ` query parameter (clause 4.19).
//!
//! Scopes form a hierarchy, and a `scopeQ` is a disjunction of conjunctions of
//! patterns over it. This module owns that syntax and the one in-memory reading
//! of it; how a pattern is spelled against a store's own path type belongs to
//! that backend.

pub mod pattern;
pub mod query;
