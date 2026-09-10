//! The collation DDL `SeaQuery` has no form for.
//!
//! `CREATE COLLATION` is a whole administrative command with no bound
//! parameters, so no query builder models it; it is spelled here, in the one
//! module of this crate sanctioned to write dialect SQL by hand.

pub mod icu_collation;
