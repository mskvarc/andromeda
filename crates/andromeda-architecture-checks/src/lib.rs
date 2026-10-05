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
pub mod detail_message;
pub mod detail_message_baseline;
pub mod god_file;
pub mod manifest_hygiene;
pub mod module_naming;
pub mod raw_constant;
pub mod raw_sql;
pub mod scope_grammar;
mod source_scan;
pub mod sql;
pub mod string_typed_error;
pub mod type_erased_error;

/// Declarative macros that generate caller-side architecture tests.
#[macro_use]
mod macros;
