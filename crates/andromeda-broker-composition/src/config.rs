//! Broker runtime configuration, split by the resource each value constructs.

mod error;
pub mod observability;
/// Aggregate configuration loading, reporting, and CLI override handling.
pub mod runtime;
mod validation;
