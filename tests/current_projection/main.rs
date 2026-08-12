//! Verification suite for the transactional current-attribute projection.
//!
//! Covers the database-schema-redesign verification-matrix rows "Current
//! projection atomicity", "Current projection delete", "Default dataset
//! uniqueness", "Distinct datasets", "Narrow Entity keys", and "Empty Entity
//! type". Each test drives the broker over HTTP and then reads
//! `current_attribute_values`, `entities`, and `temporal_entities` directly, so
//! an assertion states what the storage actually holds rather than what the
//! rendering suggests.
//!
//! Naming convention: `current_projection_<scenario>` and
//! `narrow_entity_keys_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod atomicity;
mod datasets;
mod deletion;
mod entity_keys;
mod entity_type;
mod projection_rows;
