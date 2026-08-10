//! The architecture checks shared by Andromeda's workspace crates.
//!
//! The checks cover dependency boundaries, SQL fragments, workspace-wide
//! architecture invariants, and raw constants.
//!
//! The exported declarative macros generate the caller-side tests, so failures
//! continue to identify the crate that broke its boundary.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod architecture;
pub mod dependencies;
pub mod raw_constant;
mod source_scan;
pub mod sql;

/// Declarative macros that generate caller-side architecture tests.
#[macro_use]
mod macros;
