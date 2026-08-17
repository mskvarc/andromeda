//! Verification of the UN/CEFACT unit-code catalogue end to end.
//!
//! The redesign moved `unitCode` (clause 5.2.5) out of every attribute
//! instance's metadata document and into a two-byte identity resolved through
//! the append-only catalogue `crates/andromeda-unit-catalog` compiles in and
//! migration `0009_unit_codes_seed` loads. These tests are the verification
//! matrix rows that can only be answered against a live broker and a migrated
//! tenant: that the compiled catalogue and the seeded rows are the same
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
