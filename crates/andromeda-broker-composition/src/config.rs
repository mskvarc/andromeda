//! Broker runtime configuration, split by the resource each value constructs.

mod error;
/// Composition of the durable-delivery and streaming notification configurations.
pub mod notifications;
pub mod observability;
/// Aggregate configuration loading, reporting, and CLI override handling.
pub mod runtime;
mod validation;
