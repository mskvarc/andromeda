//! Verification of the name dictionaries' capacity and their bounds.
//!
//! Migration `0007_name_dictionaries` gives both dictionary keys `INTEGER`
//! range, well past the 32,767-entry ceiling a `SMALLINT` key would impose by
//! accident, and states each dictionary's policy: byte-comparable,
//! length-bounded names plus an API-layer bound on how far one tenant's
//! dictionary may grow. The attribute dictionary is checked for capacity and
//! its bound; the entity type dictionary shares the implementation and is
//! checked for its bound.
//!
//! Every test here changes the size of a shared relation, so each holds the
//! harness's exclusive lease for its duration and removes exactly the rows it
//! created: while a dictionary sits near its bound, every other writer in the
//! fleet would be refused.
//!
//! Naming convention: `<subject>_<scenario>_<expected outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod capacity;
mod entity_type_growth_limit;
mod growth_limit;

/// The suite name every environment in this suite is allocated under.
pub const SUITE: &str = "name_dictionary";

/// Prefix of every attribute or type IRI these tests mint, so cleanup can
/// name them.
pub const GENERATED_NAME_PREFIX: &str = "https://example.test/andromeda/dictionary-verification/";
