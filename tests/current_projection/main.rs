//! Verification suite for the transactional current-state attribute rows.
//!
//! Covers current-state atomicity and deletion, default dataset uniqueness,
//! distinct datasets, narrow Entity keys, empty Entity types, replayed writes,
//! long strings, instance expiry, identifier bounds, refused-write isolation,
//! the consequences of attribute and Entity writes, the outline a deletion
//! reads, and the hoisted geometry a write leaving it alone does not rewrite.
//! Each test drives the broker over HTTP (or, for refused-write isolation, the
//! broker's own committer) and then reads
//! `current_attributes` and `entities` directly, so
//! an assertion states what the storage actually holds rather than what the
//! rendering suggests.
//!
//! Naming convention: `current_projection_<scenario>` and
//! `narrow_entity_keys_<scenario>`, except where a test states a bound or a
//! refusal as a sentence of its own (`identifier_bounds`, and the overflow
//! length check in `atomicity`).

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod atomicity;
mod attribute_writes;
mod bad_request;
mod bounded_identifier;
#[path = "../support/broker_committer.rs"]
mod broker_committer;
mod bypass_writes;
mod datasets;
mod deletion;
mod deletion_outline;
mod entity_keys;
mod entity_type;
mod entity_writes;
mod hoisted_geometry;
mod identifier_bounds;
mod instance_expiry;
mod long_text;
mod projection_rows;
mod refused_write_isolation;
mod replayed_writes;
mod write_consequences;
