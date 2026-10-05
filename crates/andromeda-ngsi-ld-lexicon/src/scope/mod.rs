//! The NGSI-LD scope grammar (ETSI GS CIM 009 clause 4.18).
//!
//! The one owner of what a Scope is: [`grammar`] is the clause 4.18 ABNF,
//! [`level`] admits a level, and [`path`] parses a whole path against the
//! grammar and its storage bounds and splits it into its levels. Every other
//! crate reads a Scope through [`path::ScopePath`].

pub mod grammar;
pub mod level;
pub mod path;
