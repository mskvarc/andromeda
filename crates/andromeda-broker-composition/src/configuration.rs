//! The broker's use of its configuration: loading it for a command and
//! admitting each section into the domain values it is read as.

/// Loading the configuration for a command and reporting the outcome.
pub mod commands;
mod error;
/// Admitting loaded sections into their domain values.
pub(crate) mod section_admission;
pub(crate) mod source_identity;
