//! Scope filtering: the `scopeQ` query parameter (clause 4.18).
//!
//! Scopes form a hierarchy that PostgreSQL stores as `ltree` label paths. This
//! module owns the query syntax, its translation to `ltree` / `lquery`, and the
//! SQL rendering against both current scopes and scope history.

pub mod ltree;
pub mod pattern;
pub mod query;
pub mod sql;
