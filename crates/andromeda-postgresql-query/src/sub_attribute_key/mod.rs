//! The equality keys of sub-attribute values, which `current_attributes`
//! stores in `sub_eq_keys` and a clause 4.9 `q` equality on a sub-attribute
//! reads through `idx_ca_sub_eq_keys`.
//!
//! A write keys every scalar value of an instance's sub-attribute tree and a
//! query keys its operand, both here, so the two sides read clause 4.6.3 and
//! the number domain by the same rules: `1.0` and `1.00` share a key, a time
//! value written as text shares the key of its value, and an array member is
//! read through its `@value`. A key selects candidates only: two values may
//! share one, so a query rechecks the value it compares.
//!
//! The hash is XXH3-64, whose output the xxHash specification fixes for every
//! platform and build, and every byte it hashes is spelled little-endian. A
//! stored key therefore stays valid across releases until the key layout
//! itself changes, which re-keys every row.

/// The key of one value at one name path.
pub mod key;
/// The keys a `q` operand reads.
pub mod operand_keys;
/// The hash of a sub-attribute's name path.
pub mod path_hash;
/// The keys of one stored string.
pub mod string_keys;
