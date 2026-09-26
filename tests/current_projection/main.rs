//! Verification suite for the transactional current-attribute projection.
//!
//! Covers the database-schema-redesign verification-matrix rows "Current
//! projection atomicity", "Current projection delete", "Default dataset
//! uniqueness", "Distinct datasets", "Narrow Entity keys", "Empty Entity
//! type", "Current projection replayed writes", "Current projection no-op
//! suppression", "Current projection long strings" and "Identifier bounds".
//! Each test drives the broker over HTTP (or, for the no-op guard, the
//! broker's own projection statement) and then reads
//! `current_attribute_values`, `entities`, and `temporal_entities` directly, so
//! an assertion states what the storage actually holds rather than what the
//! rendering suggests.
//!
//! Naming convention: `current_projection_<scenario>` and
//! `narrow_entity_keys_<scenario>`, except where a test states a bound or a
//! refusal as a sentence of its own (`identifier_bounds`, and the overflow
//! length check in `atomicity`).

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod atomicity;
mod bad_request;
mod bounded_identifier;
mod bypass_writes;
mod datasets;
mod deletion;
mod entity_keys;
mod entity_type;
mod identifier_bounds;
mod long_text;
mod noop_suppression;
mod projection_rows;
mod replayed_writes;
