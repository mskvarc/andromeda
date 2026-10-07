//! Verification of the UN/CEFACT unit-code catalogue end to end.
//!
//! `unitCode` (clause 5.2.5) is not stored in an attribute instance's metadata
//! document but as a two-byte identity resolved through an append-only
//! catalogue, which the broker compiles in and a migration seeds into
//! `unit_codes`. These tests cover the properties that can only be answered
//! against a live broker and a migrated tenant: that the compiled catalogue and the seeded rows are the same
//! assignment, that an unusable code is refused on both the API and the bypass
//! path, that the member is reconstructed byte for byte on the way out, that
//! the attribute types the specification leaves unitless cannot carry one, and
//! that an aggregate never silently spans two units.
//!
//! Naming convention: `<subject>_<scenario>_<expected outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod catalog_rows;
mod fixtures;
mod invalid_unit;
mod mixed_units;
mod reconstruction;
mod unitless_attributes;
