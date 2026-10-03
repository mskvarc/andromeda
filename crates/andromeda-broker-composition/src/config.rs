//! Broker runtime configuration, split by the resource each value constructs.

mod error;
/// Management-plane topology and metrics-exposure configuration.
pub mod management;
/// Composition of the durable-delivery and streaming notification configurations.
pub mod notifications;
pub mod observability;
pub(crate) mod pagination;
/// Aggregate configuration loading, reporting, and CLI override handling.
pub mod runtime;
mod validation;
