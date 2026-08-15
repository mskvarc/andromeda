//! Verification of the attribute-name dictionary's capacity and its bound.
//!
//! Migration `0035_attribute_names_integer` widened the dictionary key to
//! `INTEGER`, which removed the 32,767-entry ceiling the old `SMALLINT` key
//! imposed by accident, and `0039_dictionary_growth_guard` replaced that
//! accident with a stated policy: byte-comparable, length-bounded names plus an
//! API-layer bound on how far one tenant's dictionary may grow.
//!
//! Both tests here change the size of a shared relation, so each holds the
//! harness's exclusive lease for its duration and removes exactly the rows it
//! created: while a dictionary sits near its bound, every other writer in the
//! fleet would be refused.
//!
//! Naming convention: `<subject>_<scenario>_<expected outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod capacity;
mod growth_limit;

/// The suite name every environment in this suite is allocated under.
pub const SUITE: &str = "attribute_dictionary";

/// Prefix of every attribute IRI these tests mint, so cleanup can name them.
pub const GENERATED_NAME_PREFIX: &str = "https://example.test/andromeda/dictionary-verification/";
