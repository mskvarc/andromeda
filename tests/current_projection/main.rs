//! Verification suite for the transactional current-attribute projection.
//!
//! Covers the database-schema-redesign verification-matrix rows "Current
//! projection atomicity", "Current projection delete", "Default dataset
//! uniqueness", "Distinct datasets", "Narrow Entity keys", "Empty Entity
//! type", "Current projection replayed writes" and "Current projection no-op
//! suppression". Each test drives the broker over HTTP (or, for the no-op
//! guard, the broker's own projection statement) and then reads
//! `current_attribute_values`, `entities`, and `temporal_entities` directly, so
//! an assertion states what the storage actually holds rather than what the
//! rendering suggests.
//!
//! Naming convention: `current_projection_<scenario>` and
//! `narrow_entity_keys_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod atomicity;
mod datasets;
mod deletion;
mod entity_keys;
mod entity_type;
mod noop_suppression;
mod projection_rows;
mod replayed_writes;
