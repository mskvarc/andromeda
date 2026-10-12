//! Verification of the name dictionaries' capacity and their unbounded growth.
//!
//! Both dictionary keys have `INTEGER` range, well past the 32,767-entry ceiling a `SMALLINT` key would impose by
//! accident. Neither dictionary is bounded above the database: only each
//! tenant's in-process cache of a dictionary is, by bytes, and a name the cache
//! does not hold is read from its table. The attribute dictionary is checked
//! for capacity, for growth past ten thousand names, and for resolution through
//! a cache far smaller than it; the entity type dictionary shares the
//! implementation and is checked for growth.
//!
//! Every test here changes the size of a shared relation, so each holds the
//! harness's exclusive lease for its duration. The dictionaries admit an insert
//! and nothing else, so a test never removes the names it minted: each run
//! mints under a prefix of its own and never meets an earlier run's names.
//!
//! Naming convention: `<subject>_<scenario>_<expected outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod cache_eviction;
mod capacity;
mod entity_type_unbounded_growth;
mod unbounded_growth;

use uuid::Uuid;

/// The suite name every environment in this suite is allocated under.
pub const SUITE: &str = "name_dictionary";

/// Base of every attribute or type IRI these tests mint.
const GENERATED_NAME_BASE: &str = "https://example.test/andromeda/dictionary-verification/";

/// A prefix beneath [`GENERATED_NAME_BASE`] that no other run mints under.
#[must_use]
pub fn run_name_prefix() -> String {
    format!("{GENERATED_NAME_BASE}{}/", Uuid::new_v4().simple())
}
